use std::collections::HashSet;
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde::Serialize;
use serde_json::{json, Value};

use crate::db::{self, EdgeDirection, EdgeRecord, KnotCacheRecord, UpsertKnotHot};
use crate::doctor::{run_doctor, DoctorError, DoctorReport};
use crate::domain::metadata::{normalize_datetime, MetadataEntry, MetadataEntryInput};
use crate::domain::state::{InvalidStateTransition, ParseKnotStateError};
use crate::events::{
    new_event_id, now_utc_rfc3339, EventRecord, EventWriteError, EventWriter, FullEvent,
    FullEventKind, IndexEvent, IndexEventKind,
};
use crate::fsck::{run_fsck, FsckError, FsckReport};
use crate::hierarchy_alias::{build_alias_maps, AliasMaps};
use crate::imports::{ImportError, ImportService, ImportStatus, ImportSummary};
use crate::knot_id::generate_knot_id;
use crate::locks::{FileLock, LockError};
use crate::perf::{run_perf_harness, PerfError, PerfReport};
use crate::remote_init::{init_remote_knots_branch, RemoteInitError};
use crate::replication::{PushSummary, ReplicationService, ReplicationSummary};
use crate::snapshots::{write_snapshots, SnapshotError, SnapshotWriteSummary};
use crate::sync::{SyncError, SyncSummary};
use crate::workflow::{WorkflowDefinition, WorkflowError, WorkflowRegistry};

pub struct App {
    conn: Connection,
    writer: EventWriter,
    repo_root: PathBuf,
    workflow_registry: WorkflowRegistry,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct KnotView {
    pub id: String,
    pub alias: Option<String>,
    pub title: String,
    pub state: String,
    pub updated_at: String,
    pub body: Option<String>,
    pub description: Option<String>,
    pub priority: Option<i64>,
    #[serde(rename = "type")]
    pub knot_type: Option<String>,
    pub tags: Vec<String>,
    pub notes: Vec<MetadataEntry>,
    pub handoff_capsules: Vec<MetadataEntry>,
    pub workflow_id: String,
    pub workflow_etag: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UpdateKnotPatch {
    pub title: Option<String>,
    pub description: Option<String>,
    pub priority: Option<i64>,
    pub status: Option<String>,
    pub knot_type: Option<String>,
    pub add_tags: Vec<String>,
    pub remove_tags: Vec<String>,
    pub add_note: Option<MetadataEntryInput>,
    pub add_handoff_capsule: Option<MetadataEntryInput>,
    pub expected_workflow_etag: Option<String>,
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EdgeView {
    pub src: String,
    pub kind: String,
    pub dst: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ColdKnotView {
    pub id: String,
    pub title: String,
    pub state: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncPolicy {
    Auto,
    Always,
    Never,
}

impl UpdateKnotPatch {
    fn has_changes(&self) -> bool {
        self.title.is_some()
            || self.description.is_some()
            || self.priority.is_some()
            || self.status.is_some()
            || self.knot_type.is_some()
            || !self.add_tags.is_empty()
            || !self.remove_tags.is_empty()
            || self.add_note.is_some()
            || self.add_handoff_capsule.is_some()
    }
}

impl App {
    pub fn open(db_path: &str, repo_root: PathBuf) -> Result<Self, AppError> {
        ensure_parent_dir(db_path)?;
        let conn = db::open_connection(db_path)?;
        let workflow_registry = WorkflowRegistry::load(&repo_root)?;
        let writer = EventWriter::new(repo_root.clone());
        Ok(Self {
            conn,
            writer,
            repo_root,
            workflow_registry,
        })
    }

    fn repo_lock_path(&self) -> PathBuf {
        self.repo_root
            .join(".knots")
            .join("locks")
            .join("repo.lock")
    }

    fn cache_lock_path(&self) -> PathBuf {
        self.repo_root
            .join(".knots")
            .join("cache")
            .join("cache.lock")
    }

    fn read_sync_policy(&self) -> Result<SyncPolicy, AppError> {
        let raw = db::get_meta(&self.conn, "sync_policy")?.unwrap_or_else(|| "auto".to_string());
        match raw.trim().to_ascii_lowercase().as_str() {
            "always" => Ok(SyncPolicy::Always),
            "never" => Ok(SyncPolicy::Never),
            _ => Ok(SyncPolicy::Auto),
        }
    }

    fn read_sync_budget_ms(&self) -> Result<u64, AppError> {
        let raw =
            db::get_meta(&self.conn, "sync_auto_budget_ms")?.unwrap_or_else(|| "750".to_string());
        let budget = raw.trim().parse::<u64>().unwrap_or(750);
        Ok(budget)
    }

    fn mark_sync_pending(&self) -> Result<(), AppError> {
        db::set_meta(&self.conn, "sync_pending", "true")?;
        Ok(())
    }

    fn maybe_auto_sync_for_read(&self) -> Result<(), AppError> {
        match self.read_sync_policy()? {
            SyncPolicy::Never => Ok(()),
            SyncPolicy::Always => {
                let _ = self.pull()?;
                Ok(())
            }
            SyncPolicy::Auto => {
                let repo_lock = FileLock::try_acquire(&self.repo_lock_path())?;
                let Some(_repo_guard) = repo_lock else {
                    return self.mark_sync_pending();
                };
                let _cache_guard =
                    FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5000))?;
                let start = Instant::now();
                if let Err(err) = self.pull_unlocked() {
                    return match err {
                        AppError::Sync(SyncError::GitCommandFailed { .. })
                        | AppError::Sync(SyncError::GitUnavailable) => {
                            self.mark_sync_pending()?;
                            Ok(())
                        }
                        other => Err(other),
                    };
                }
                if start.elapsed().as_millis() > self.read_sync_budget_ms()? as u128 {
                    self.mark_sync_pending()?;
                }
                Ok(())
            }
        }
    }

    fn pull_unlocked(&self) -> Result<SyncSummary, AppError> {
        let service = ReplicationService::new(&self.conn, self.repo_root.clone());
        Ok(service.pull()?)
    }

    fn known_knot_ids(&self) -> Result<HashSet<String>, AppError> {
        let mut ids = HashSet::new();
        for record in db::list_knot_hot(&self.conn)? {
            ids.insert(record.id);
        }
        for record in db::list_knot_warm(&self.conn)? {
            ids.insert(record.id);
        }
        for record in db::list_cold_catalog(&self.conn)? {
            ids.insert(record.id);
        }
        Ok(ids)
    }

    fn alias_maps(&self) -> Result<AliasMaps, AppError> {
        let mut ids = self.known_knot_ids()?;
        let parent_edges = db::list_edges_by_kind(&self.conn, "parent_of")?;
        let mut edges = Vec::new();
        for edge in parent_edges {
            ids.insert(edge.src.clone());
            ids.insert(edge.dst.clone());
            edges.push((edge.src, edge.dst));
        }
        Ok(build_alias_maps(ids.into_iter().collect(), &edges))
    }

    fn resolve_knot_token(&self, token: &str) -> Result<String, AppError> {
        if token.trim().is_empty() {
            return Ok(token.to_string());
        }
        let maps = self.alias_maps()?;
        Ok(maps
            .alias_to_id
            .get(token)
            .cloned()
            .unwrap_or_else(|| token.to_string()))
    }

    fn with_alias_maps(knot: KnotView, maps: &AliasMaps) -> KnotView {
        let mut knot = knot;
        let alias = maps.id_to_alias.get(&knot.id).cloned();
        knot.alias = alias.filter(|value| value != &knot.id);
        knot
    }

    fn apply_aliases_to_knots(&self, knots: Vec<KnotView>) -> Result<Vec<KnotView>, AppError> {
        let maps = self.alias_maps()?;
        Ok(knots
            .into_iter()
            .map(|knot| Self::with_alias_maps(knot, &maps))
            .collect())
    }

    fn apply_alias_to_knot(&self, knot: KnotView) -> Result<KnotView, AppError> {
        let maps = self.alias_maps()?;
        Ok(Self::with_alias_maps(knot, &maps))
    }

    fn next_knot_id(&self) -> Result<String, AppError> {
        let existing = self.known_knot_ids()?;
        Ok(generate_knot_id(&self.repo_root, |candidate| {
            existing.contains(candidate)
        }))
    }

    pub fn list_workflows(&self) -> Vec<WorkflowDefinition> {
        self.workflow_registry.list()
    }

    pub fn show_workflow(&self, id: &str) -> Result<WorkflowDefinition, AppError> {
        Ok(self.workflow_registry.require(id)?.clone())
    }

    fn resolve_workflow_for_record<'a>(
        &'a self,
        record: &KnotCacheRecord,
    ) -> Result<&'a WorkflowDefinition, AppError> {
        let workflow_id = non_empty(record.workflow_id.as_str()).ok_or_else(|| {
            AppError::InvalidArgument(format!("knot '{}' is missing workflow_id", record.id))
        })?;
        Ok(self.workflow_registry.require(&workflow_id)?)
    }

    pub fn create_knot(
        &self,
        title: &str,
        body: Option<&str>,
        initial_state: Option<&str>,
        workflow_id: Option<&str>,
    ) -> Result<KnotView, AppError> {
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(30_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(30_000))?;
        let workflow = self.workflow_registry.resolve(workflow_id)?;
        let state = non_empty(initial_state.unwrap_or(""))
            .unwrap_or_else(|| workflow.initial_state.clone())
            .to_ascii_lowercase();
        workflow.require_state(&state)?;
        let knot_id = self.next_knot_id()?;
        let occurred_at = now_utc_rfc3339();

        let full_event = FullEvent::with_identity(
            new_event_id(),
            occurred_at.clone(),
            knot_id.clone(),
            FullEventKind::KnotCreated.as_str(),
            json!({
                "title": title,
                "state": state.as_str(),
                "workflow_id": workflow.id.as_str(),
                "body": body,
            }),
        );

        let index_event_id = new_event_id();
        let idx_event = IndexEvent::with_identity(
            index_event_id.clone(),
            occurred_at.clone(),
            IndexEventKind::KnotHead.as_str(),
            json!({
                "knot_id": knot_id,
                "title": title,
                "state": state.as_str(),
                "workflow_id": workflow.id.as_str(),
                "updated_at": occurred_at,
                "terminal": workflow.is_terminal_state(&state),
            }),
        );

        self.writer.write(&EventRecord::full(full_event))?;
        self.writer.write(&EventRecord::index(idx_event))?;

        db::upsert_knot_hot(
            &self.conn,
            &UpsertKnotHot {
                id: &knot_id,
                title,
                state: state.as_str(),
                updated_at: &occurred_at,
                body,
                description: body,
                priority: None,
                knot_type: None,
                tags: &[],
                notes: &[],
                handoff_capsules: &[],
                workflow_id: workflow.id.as_str(),
                workflow_etag: Some(&index_event_id),
                created_at: Some(&occurred_at),
            },
        )?;
        let record = db::get_knot_hot(&self.conn, &knot_id)?
            .ok_or_else(|| AppError::NotFound(knot_id.clone()))?;
        self.apply_alias_to_knot(KnotView::from(record))
    }

    pub fn set_state(
        &self,
        id: &str,
        next_state: &str,
        force: bool,
        expected_workflow_etag: Option<&str>,
    ) -> Result<KnotView, AppError> {
        let id = self.resolve_knot_token(id)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(30_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(30_000))?;
        let current =
            db::get_knot_hot(&self.conn, &id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
        ensure_workflow_etag(&current, expected_workflow_etag)?;
        let workflow = self.resolve_workflow_for_record(&current)?;
        let next = next_state.trim().to_ascii_lowercase();
        workflow.validate_transition(&current.state, &next, force)?;

        let occurred_at = now_utc_rfc3339();
        let mut full_event = FullEvent::with_identity(
            new_event_id(),
            occurred_at.clone(),
            id.clone(),
            FullEventKind::KnotStateSet.as_str(),
            json!({
                "from": &current.state,
                "to": &next,
                "workflow_id": &current.workflow_id,
                "force": force,
            }),
        );
        if let Some(expected) = expected_workflow_etag {
            full_event = full_event.with_precondition(expected);
        }

        let index_event_id = new_event_id();
        let mut idx_event = IndexEvent::with_identity(
            index_event_id.clone(),
            occurred_at.clone(),
            IndexEventKind::KnotHead.as_str(),
            json!({
                "knot_id": id,
                "title": current.title,
                "state": &next,
                "workflow_id": &current.workflow_id,
                "updated_at": occurred_at,
                "terminal": workflow.is_terminal_state(&next),
            }),
        );
        if let Some(expected) = expected_workflow_etag {
            idx_event = idx_event.with_precondition(expected);
        }

        self.writer.write(&EventRecord::full(full_event))?;
        self.writer.write(&EventRecord::index(idx_event))?;

        db::upsert_knot_hot(
            &self.conn,
            &UpsertKnotHot {
                id: &id,
                title: &current.title,
                state: &next,
                updated_at: &occurred_at,
                body: current.body.as_deref(),
                description: current.description.as_deref(),
                priority: current.priority,
                knot_type: current.knot_type.as_deref(),
                tags: &current.tags,
                notes: &current.notes,
                handoff_capsules: &current.handoff_capsules,
                workflow_id: &current.workflow_id,
                workflow_etag: Some(&index_event_id),
                created_at: current.created_at.as_deref(),
            },
        )?;
        let updated =
            db::get_knot_hot(&self.conn, &id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
        self.apply_alias_to_knot(KnotView::from(updated))
    }

    pub fn update_knot(&self, id: &str, patch: UpdateKnotPatch) -> Result<KnotView, AppError> {
        let id = self.resolve_knot_token(id)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(30_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(30_000))?;
        if !patch.has_changes() {
            return Err(AppError::InvalidArgument(
                "update requires at least one field change".to_string(),
            ));
        }

        let current =
            db::get_knot_hot(&self.conn, &id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
        ensure_workflow_etag(&current, patch.expected_workflow_etag.as_deref())?;
        let mut title = current.title.clone();
        let mut state = current.state.clone();
        let mut description = current.description.clone();
        let mut body = current.body.clone();
        let mut priority = current.priority;
        let mut knot_type = current.knot_type.clone();
        let workflow = self.resolve_workflow_for_record(&current)?;
        let mut tags = current.tags.clone();
        let mut notes = current.notes.clone();
        let mut handoff_capsules = current.handoff_capsules.clone();
        let occurred_at = now_utc_rfc3339();
        let mut full_events = Vec::new();

        if let Some(next_state_raw) = patch.status.as_deref() {
            let next_state = next_state_raw.trim().to_ascii_lowercase();
            workflow.validate_transition(&state, &next_state, patch.force)?;
            if state != next_state {
                state = next_state;
                full_events.push(FullEvent::with_identity(
                    new_event_id(),
                    occurred_at.clone(),
                    id.to_string(),
                    FullEventKind::KnotStateSet.as_str(),
                    json!({
                        "from": &current.state,
                        "to": &state,
                        "workflow_id": &current.workflow_id,
                        "force": patch.force,
                    }),
                ));
            }
        }

        if let Some(next_title_raw) = patch.title.as_deref() {
            let next_title = next_title_raw.trim();
            if next_title.is_empty() {
                return Err(AppError::InvalidArgument(
                    "title cannot be empty".to_string(),
                ));
            }
            if next_title != title {
                full_events.push(FullEvent::with_identity(
                    new_event_id(),
                    occurred_at.clone(),
                    id.to_string(),
                    FullEventKind::KnotTitleSet.as_str(),
                    json!({
                        "from": &title,
                        "to": next_title,
                    }),
                ));
                title = next_title.to_string();
            }
        }

        if let Some(next_description_raw) = patch.description.as_deref() {
            let next_description = non_empty(next_description_raw);
            if next_description != description {
                full_events.push(FullEvent::with_identity(
                    new_event_id(),
                    occurred_at.clone(),
                    id.to_string(),
                    FullEventKind::KnotDescriptionSet.as_str(),
                    json!({
                        "description": next_description,
                    }),
                ));
                description = next_description;
                body = description.clone();
            }
        }

        if let Some(next_priority) = patch.priority {
            if !(0..=4).contains(&next_priority) {
                return Err(AppError::InvalidArgument(
                    "priority must be between 0 and 4".to_string(),
                ));
            }
            if priority != Some(next_priority) {
                full_events.push(FullEvent::with_identity(
                    new_event_id(),
                    occurred_at.clone(),
                    id.to_string(),
                    FullEventKind::KnotPrioritySet.as_str(),
                    json!({
                        "priority": next_priority,
                    }),
                ));
                priority = Some(next_priority);
            }
        }

        if let Some(next_type_raw) = patch.knot_type.as_deref() {
            let next_type = non_empty(next_type_raw);
            if next_type != knot_type {
                full_events.push(FullEvent::with_identity(
                    new_event_id(),
                    occurred_at.clone(),
                    id.to_string(),
                    FullEventKind::KnotTypeSet.as_str(),
                    json!({
                        "type": next_type,
                    }),
                ));
                knot_type = next_type;
            }
        }

        for tag in &patch.add_tags {
            let normalized = normalize_tag(tag);
            if normalized.is_empty() {
                continue;
            }
            if !tags.iter().any(|existing| existing == &normalized) {
                tags.push(normalized.clone());
                full_events.push(FullEvent::with_identity(
                    new_event_id(),
                    occurred_at.clone(),
                    id.to_string(),
                    FullEventKind::KnotTagAdd.as_str(),
                    json!({ "tag": normalized }),
                ));
            }
        }

        for tag in &patch.remove_tags {
            let normalized = normalize_tag(tag);
            if normalized.is_empty() {
                continue;
            }
            if tags.iter().any(|existing| existing == &normalized) {
                tags.retain(|existing| existing != &normalized);
                full_events.push(FullEvent::with_identity(
                    new_event_id(),
                    occurred_at.clone(),
                    id.to_string(),
                    FullEventKind::KnotTagRemove.as_str(),
                    json!({ "tag": normalized }),
                ));
            }
        }

        if let Some(input) = patch.add_note {
            let entry = metadata_entry_from_input(input, &occurred_at)?;
            if !notes
                .iter()
                .any(|existing| existing.entry_id == entry.entry_id)
            {
                notes.push(entry.clone());
                full_events.push(FullEvent::with_identity(
                    new_event_id(),
                    occurred_at.clone(),
                    id.to_string(),
                    FullEventKind::KnotNoteAdded.as_str(),
                    json!({
                        "entry_id": entry.entry_id,
                        "content": entry.content,
                        "username": entry.username,
                        "datetime": entry.datetime,
                        "agentname": entry.agentname,
                        "model": entry.model,
                        "version": entry.version,
                    }),
                ));
            }
        }

        if let Some(input) = patch.add_handoff_capsule {
            let entry = metadata_entry_from_input(input, &occurred_at)?;
            if !handoff_capsules
                .iter()
                .any(|existing| existing.entry_id == entry.entry_id)
            {
                handoff_capsules.push(entry.clone());
                full_events.push(FullEvent::with_identity(
                    new_event_id(),
                    occurred_at.clone(),
                    id.to_string(),
                    FullEventKind::KnotHandoffCapsuleAdded.as_str(),
                    json!({
                        "entry_id": entry.entry_id,
                        "content": entry.content,
                        "username": entry.username,
                        "datetime": entry.datetime,
                        "agentname": entry.agentname,
                        "model": entry.model,
                        "version": entry.version,
                    }),
                ));
            }
        }

        if full_events.is_empty() {
            return self.apply_alias_to_knot(KnotView::from(current));
        }

        for mut event in full_events {
            if let Some(expected) = patch.expected_workflow_etag.as_deref() {
                event = event.with_precondition(expected);
            }
            self.writer.write(&EventRecord::full(event))?;
        }

        let terminal = workflow.is_terminal_state(&state);
        let index_event_id = new_event_id();
        let mut idx_event = IndexEvent::with_identity(
            index_event_id.clone(),
            occurred_at.clone(),
            IndexEventKind::KnotHead.as_str(),
            json!({
                "knot_id": id,
                "title": &title,
                "state": &state,
                "workflow_id": &current.workflow_id,
                "updated_at": occurred_at,
                "terminal": terminal,
            }),
        );
        if let Some(expected) = patch.expected_workflow_etag.as_deref() {
            idx_event = idx_event.with_precondition(expected);
        }
        self.writer.write(&EventRecord::index(idx_event))?;

        db::upsert_knot_hot(
            &self.conn,
            &UpsertKnotHot {
                id: &id,
                title: &title,
                state: &state,
                updated_at: &occurred_at,
                body: body.as_deref(),
                description: description.as_deref(),
                priority,
                knot_type: knot_type.as_deref(),
                tags: &tags,
                notes: &notes,
                handoff_capsules: &handoff_capsules,
                workflow_id: &current.workflow_id,
                workflow_etag: Some(&index_event_id),
                created_at: current.created_at.as_deref(),
            },
        )?;
        let updated =
            db::get_knot_hot(&self.conn, &id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
        self.apply_alias_to_knot(KnotView::from(updated))
    }

    pub fn list_knots(&self) -> Result<Vec<KnotView>, AppError> {
        self.maybe_auto_sync_for_read()?;
        let knots = db::list_knot_hot(&self.conn)?
            .into_iter()
            .map(KnotView::from)
            .collect();
        self.apply_aliases_to_knots(knots)
    }

    pub fn show_knot(&self, id: &str) -> Result<Option<KnotView>, AppError> {
        let id = self.resolve_knot_token(id)?;
        self.maybe_auto_sync_for_read()?;
        if let Some(knot) = db::get_knot_hot(&self.conn, &id)? {
            return Ok(Some(self.apply_alias_to_knot(KnotView::from(knot))?));
        }
        self.rehydrate(&id)
    }

    pub fn pull(&self) -> Result<SyncSummary, AppError> {
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(30_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(30_000))?;
        self.pull_unlocked()
    }

    pub fn push(&self) -> Result<PushSummary, AppError> {
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(30_000))?;
        let service = ReplicationService::new(&self.conn, self.repo_root.clone());
        Ok(service.push()?)
    }

    pub fn sync(&self) -> Result<ReplicationSummary, AppError> {
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(30_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(30_000))?;
        let service = ReplicationService::new(&self.conn, self.repo_root.clone());
        Ok(service.sync()?)
    }

    pub fn init_remote(&self) -> Result<(), AppError> {
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(30_000))?;
        init_remote_knots_branch(&self.repo_root)?;
        Ok(())
    }

    pub fn fsck(&self) -> Result<FsckReport, AppError> {
        Ok(run_fsck(&self.repo_root)?)
    }

    pub fn doctor(&self) -> Result<DoctorReport, AppError> {
        Ok(run_doctor(&self.repo_root)?)
    }

    pub fn compact_write_snapshots(&self) -> Result<SnapshotWriteSummary, AppError> {
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(30_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(30_000))?;
        Ok(write_snapshots(&self.conn, &self.repo_root)?)
    }

    pub fn perf_harness(&self, iterations: u32) -> Result<PerfReport, AppError> {
        let _ = self;
        Ok(run_perf_harness(iterations)?)
    }

    pub fn add_edge(&self, src: &str, kind: &str, dst: &str) -> Result<EdgeView, AppError> {
        let src = self.resolve_knot_token(src)?;
        let dst = self.resolve_knot_token(dst)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(30_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(30_000))?;
        self.apply_edge_change(&src, kind, &dst, true)
    }

    pub fn remove_edge(&self, src: &str, kind: &str, dst: &str) -> Result<EdgeView, AppError> {
        let src = self.resolve_knot_token(src)?;
        let dst = self.resolve_knot_token(dst)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(30_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(30_000))?;
        self.apply_edge_change(&src, kind, &dst, false)
    }

    pub fn list_edges(&self, id: &str, direction: &str) -> Result<Vec<EdgeView>, AppError> {
        let id = self.resolve_knot_token(id)?;
        let direction = parse_edge_direction(direction)?;
        let rows = db::list_edges(&self.conn, &id, direction)?;
        Ok(rows.into_iter().map(EdgeView::from).collect())
    }

    pub fn list_layout_edges(&self) -> Result<Vec<EdgeView>, AppError> {
        let mut rows = db::list_edges_by_kind(&self.conn, "parent_of")?;
        rows.extend(db::list_edges_by_kind(&self.conn, "blocked_by")?);
        rows.extend(db::list_edges_by_kind(&self.conn, "blocks")?);
        Ok(rows.into_iter().map(EdgeView::from).collect())
    }

    pub fn import_jsonl(
        &self,
        file: &str,
        since: Option<&str>,
        dry_run: bool,
    ) -> Result<ImportSummary, AppError> {
        let service = ImportService::new(&self.conn, &self.writer);
        Ok(service.import_jsonl(file, since, dry_run)?)
    }

    pub fn import_dolt(
        &self,
        repo: &str,
        since: Option<&str>,
        dry_run: bool,
    ) -> Result<ImportSummary, AppError> {
        let service = ImportService::new(&self.conn, &self.writer);
        Ok(service.import_dolt(repo, since, dry_run)?)
    }

    pub fn import_statuses(&self) -> Result<Vec<ImportStatus>, AppError> {
        let service = ImportService::new(&self.conn, &self.writer);
        Ok(service.list_statuses()?)
    }

    pub fn cold_sync(&self) -> Result<SyncSummary, AppError> {
        self.pull()
    }

    pub fn cold_search(&self, term: &str) -> Result<Vec<ColdKnotView>, AppError> {
        self.maybe_auto_sync_for_read()?;
        Ok(db::search_cold_catalog(&self.conn, term)?
            .into_iter()
            .map(|record| ColdKnotView {
                id: record.id,
                title: record.title,
                state: record.state,
                updated_at: record.updated_at,
            })
            .collect())
    }

    pub fn rehydrate(&self, id: &str) -> Result<Option<KnotView>, AppError> {
        let id = self.resolve_knot_token(id)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(30_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(30_000))?;
        if let Some(knot) = db::get_knot_hot(&self.conn, &id)? {
            return Ok(Some(self.apply_alias_to_knot(KnotView::from(knot))?));
        }
        let warm = db::get_knot_warm(&self.conn, &id)?;
        let cold = db::get_cold_catalog(&self.conn, &id)?;
        let title = warm
            .as_ref()
            .map(|record| record.title.clone())
            .or_else(|| cold.as_ref().map(|record| record.title.clone()));
        let Some(title) = title else {
            return Ok(None);
        };
        let state = cold
            .as_ref()
            .map(|record| record.state.clone())
            .unwrap_or_else(|| "work_item".to_string());
        let updated_at = cold
            .as_ref()
            .map(|record| record.updated_at.clone())
            .unwrap_or_else(now_utc_rfc3339);
        let record = rehydrate_from_events(&self.repo_root, &id, title, state, updated_at)?;
        db::upsert_knot_hot(
            &self.conn,
            &UpsertKnotHot {
                id: &id,
                title: &record.title,
                state: &record.state,
                updated_at: &record.updated_at,
                body: record.body.as_deref(),
                description: record.description.as_deref(),
                priority: record.priority,
                knot_type: record.knot_type.as_deref(),
                tags: &record.tags,
                notes: &record.notes,
                handoff_capsules: &record.handoff_capsules,
                workflow_id: &record.workflow_id,
                workflow_etag: record.workflow_etag.as_deref(),
                created_at: record.created_at.as_deref(),
            },
        )?;
        let hot =
            db::get_knot_hot(&self.conn, &id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
        Ok(Some(self.apply_alias_to_knot(KnotView::from(hot))?))
    }

    fn apply_edge_change(
        &self,
        src: &str,
        kind: &str,
        dst: &str,
        add: bool,
    ) -> Result<EdgeView, AppError> {
        if src.trim().is_empty() || kind.trim().is_empty() || dst.trim().is_empty() {
            return Err(AppError::InvalidArgument(
                "src, kind, and dst are required".to_string(),
            ));
        }

        let current = db::get_knot_hot(&self.conn, src)?
            .ok_or_else(|| AppError::NotFound(src.to_string()))?;
        let occurred_at = now_utc_rfc3339();
        let full_kind = if add {
            FullEventKind::KnotEdgeAdd
        } else {
            FullEventKind::KnotEdgeRemove
        };
        let full_event = FullEvent::with_identity(
            new_event_id(),
            occurred_at.clone(),
            src.to_string(),
            full_kind.as_str(),
            json!({
                "kind": kind,
                "dst": dst,
            }),
        );
        self.writer.write(&EventRecord::full(full_event))?;

        let workflow = self.resolve_workflow_for_record(&current)?;
        let terminal = workflow.is_terminal_state(&current.state);
        let index_event_id = new_event_id();
        let idx_event = IndexEvent::with_identity(
            index_event_id.clone(),
            occurred_at.clone(),
            IndexEventKind::KnotHead.as_str(),
            json!({
                "knot_id": src,
                "title": current.title,
                "state": current.state,
                "workflow_id": &current.workflow_id,
                "updated_at": occurred_at,
                "terminal": terminal,
            }),
        );
        self.writer.write(&EventRecord::index(idx_event))?;

        if add {
            db::insert_edge(&self.conn, src, kind, dst)?;
        } else {
            db::delete_edge(&self.conn, src, kind, dst)?;
        }

        db::upsert_knot_hot(
            &self.conn,
            &UpsertKnotHot {
                id: src,
                title: &current.title,
                state: &current.state,
                updated_at: &occurred_at,
                body: current.body.as_deref(),
                description: current.description.as_deref(),
                priority: current.priority,
                knot_type: current.knot_type.as_deref(),
                tags: &current.tags,
                notes: &current.notes,
                handoff_capsules: &current.handoff_capsules,
                workflow_id: &current.workflow_id,
                workflow_etag: Some(&index_event_id),
                created_at: current.created_at.as_deref(),
            },
        )?;
        Ok(EdgeView {
            src: src.to_string(),
            kind: kind.to_string(),
            dst: dst.to_string(),
        })
    }
}

fn ensure_parent_dir(path: &str) -> Result<(), AppError> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn parse_edge_direction(raw: &str) -> Result<EdgeDirection, AppError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "incoming" | "in" => Ok(EdgeDirection::Incoming),
        "outgoing" | "out" => Ok(EdgeDirection::Outgoing),
        "both" | "all" => Ok(EdgeDirection::Both),
        _ => Err(AppError::InvalidArgument(format!(
            "unsupported edge direction '{}'; use incoming|outgoing|both",
            raw
        ))),
    }
}

fn non_empty(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn normalize_tag(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

fn metadata_entry_from_input(
    input: MetadataEntryInput,
    fallback_datetime: &str,
) -> Result<MetadataEntry, AppError> {
    if input.content.trim().is_empty() {
        return Err(AppError::InvalidArgument(
            "metadata content cannot be empty".to_string(),
        ));
    }
    if let Some(raw) = input.datetime.as_deref() {
        if normalize_datetime(Some(raw)).is_none() {
            return Err(AppError::InvalidArgument(
                "metadata datetime must be RFC3339".to_string(),
            ));
        }
    }
    Ok(MetadataEntry::from_input(input, fallback_datetime))
}

fn ensure_workflow_etag(
    current: &KnotCacheRecord,
    expected_workflow_etag: Option<&str>,
) -> Result<(), AppError> {
    let Some(expected) = expected_workflow_etag else {
        return Ok(());
    };
    let current_etag = current.workflow_etag.as_deref().unwrap_or("");
    if current_etag == expected {
        return Ok(());
    }
    Err(AppError::StaleWorkflowHead {
        expected: expected.to_string(),
        current: current_etag.to_string(),
    })
}

struct RehydrateProjection {
    title: String,
    state: String,
    updated_at: String,
    body: Option<String>,
    description: Option<String>,
    priority: Option<i64>,
    knot_type: Option<String>,
    tags: Vec<String>,
    notes: Vec<MetadataEntry>,
    handoff_capsules: Vec<MetadataEntry>,
    workflow_id: String,
    workflow_etag: Option<String>,
    created_at: Option<String>,
}

fn rehydrate_from_events(
    repo_root: &std::path::Path,
    knot_id: &str,
    title: String,
    state: String,
    updated_at: String,
) -> Result<RehydrateProjection, AppError> {
    let mut projection = RehydrateProjection {
        title,
        state,
        updated_at: updated_at.clone(),
        body: None,
        description: None,
        priority: None,
        knot_type: None,
        tags: Vec::new(),
        notes: Vec::new(),
        handoff_capsules: Vec::new(),
        workflow_id: String::new(),
        workflow_etag: None,
        created_at: Some(updated_at),
    };

    let mut stack = vec![repo_root.join(".knots").join("events")];
    let mut full_paths = Vec::new();
    while let Some(dir) = stack.pop() {
        if !dir.exists() {
            continue;
        }
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "json") {
                full_paths.push(path);
            }
        }
    }
    full_paths.sort();

    for path in full_paths {
        let bytes = fs::read(&path)?;
        let event: FullEvent = serde_json::from_slice(&bytes).map_err(|err| {
            AppError::InvalidArgument(format!(
                "invalid rehydrate event '{}': {}",
                path.display(),
                err
            ))
        })?;
        if event.knot_id != knot_id {
            continue;
        }
        apply_rehydrate_event(&mut projection, &event);
    }

    let mut idx_stack = vec![repo_root.join(".knots").join("index")];
    let mut idx_paths = Vec::new();
    while let Some(dir) = idx_stack.pop() {
        if !dir.exists() {
            continue;
        }
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.is_dir() {
                idx_stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "json") {
                idx_paths.push(path);
            }
        }
    }
    idx_paths.sort();
    for path in idx_paths {
        let bytes = fs::read(&path)?;
        let event: IndexEvent = serde_json::from_slice(&bytes).map_err(|err| {
            AppError::InvalidArgument(format!(
                "invalid rehydrate index '{}': {}",
                path.display(),
                err
            ))
        })?;
        if event.event_type != IndexEventKind::KnotHead.as_str() {
            continue;
        }
        let Some(data) = event.data.as_object() else {
            continue;
        };
        if data.get("knot_id").and_then(Value::as_str) != Some(knot_id) {
            continue;
        }
        if let Some(title) = data.get("title").and_then(Value::as_str) {
            projection.title = title.to_string();
        }
        if let Some(state) = data.get("state").and_then(Value::as_str) {
            projection.state = state.to_string();
        }
        if let Some(updated_at) = data.get("updated_at").and_then(Value::as_str) {
            projection.updated_at = updated_at.to_string();
        }
        if let Some(workflow_id) = data.get("workflow_id").and_then(Value::as_str) {
            projection.workflow_id = workflow_id.trim().to_ascii_lowercase();
        }
        projection.workflow_etag = Some(event.event_id.clone());
    }

    if projection.workflow_id.trim().is_empty() {
        return Err(AppError::InvalidArgument(format!(
            "rehydrate events for '{}' are missing workflow_id",
            knot_id
        )));
    }

    Ok(projection)
}

fn apply_rehydrate_event(projection: &mut RehydrateProjection, event: &FullEvent) {
    let Some(data) = event.data.as_object() else {
        return;
    };

    match event.event_type.as_str() {
        "knot.created" => {
            if let Some(title) = data.get("title").and_then(Value::as_str) {
                projection.title = title.to_string();
            }
            if let Some(state) = data.get("state").and_then(Value::as_str) {
                projection.state = state.to_string();
            }
            if let Some(workflow_id) = data.get("workflow_id").and_then(Value::as_str) {
                projection.workflow_id = workflow_id.trim().to_ascii_lowercase();
            }
            projection.created_at = Some(event.occurred_at.clone());
            projection.updated_at = event.occurred_at.clone();
        }
        "knot.title_set" => {
            if let Some(value) = data.get("to").and_then(Value::as_str) {
                projection.title = value.to_string();
                projection.updated_at = event.occurred_at.clone();
            }
        }
        "knot.state_set" => {
            if let Some(value) = data.get("to").and_then(Value::as_str) {
                projection.state = value.to_string();
                projection.updated_at = event.occurred_at.clone();
            }
        }
        "knot.description_set" => {
            let next = data
                .get("description")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string);
            projection.description = next.clone();
            projection.body = next;
            projection.updated_at = event.occurred_at.clone();
        }
        "knot.priority_set" => {
            projection.priority = data.get("priority").and_then(Value::as_i64);
            projection.updated_at = event.occurred_at.clone();
        }
        "knot.type_set" => {
            projection.knot_type = data
                .get("type")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string);
            projection.updated_at = event.occurred_at.clone();
        }
        "knot.tag_add" => {
            if let Some(tag) = data.get("tag").and_then(Value::as_str) {
                let normalized = tag.trim().to_ascii_lowercase();
                if !normalized.is_empty()
                    && !projection
                        .tags
                        .iter()
                        .any(|existing| existing == &normalized)
                {
                    projection.tags.push(normalized);
                }
            }
        }
        "knot.tag_remove" => {
            if let Some(tag) = data.get("tag").and_then(Value::as_str) {
                let normalized = tag.trim().to_ascii_lowercase();
                projection.tags.retain(|existing| existing != &normalized);
            }
        }
        "knot.note_added" => {
            if let Some(entry) = parse_metadata_entry_for_rehydrate(data) {
                if !projection
                    .notes
                    .iter()
                    .any(|existing| existing.entry_id == entry.entry_id)
                {
                    projection.notes.push(entry);
                }
            }
        }
        "knot.handoff_capsule_added" => {
            if let Some(entry) = parse_metadata_entry_for_rehydrate(data) {
                if !projection
                    .handoff_capsules
                    .iter()
                    .any(|existing| existing.entry_id == entry.entry_id)
                {
                    projection.handoff_capsules.push(entry);
                }
            }
        }
        _ => {}
    }
}

fn parse_metadata_entry_for_rehydrate(
    data: &serde_json::Map<String, Value>,
) -> Option<MetadataEntry> {
    let entry_id = data.get("entry_id")?.as_str()?.to_string();
    let content = data.get("content")?.as_str()?.to_string();
    let username = data.get("username")?.as_str()?.to_string();
    let datetime = data.get("datetime")?.as_str()?.to_string();
    let agentname = data.get("agentname")?.as_str()?.to_string();
    let model = data.get("model")?.as_str()?.to_string();
    let version = data.get("version")?.as_str()?.to_string();
    Some(MetadataEntry {
        entry_id,
        content,
        username,
        datetime,
        agentname,
        model,
        version,
    })
}

impl From<KnotCacheRecord> for KnotView {
    fn from(value: KnotCacheRecord) -> Self {
        Self {
            id: value.id,
            alias: None,
            title: value.title,
            state: value.state,
            updated_at: value.updated_at,
            body: value.body,
            description: value.description,
            priority: value.priority,
            knot_type: value.knot_type,
            tags: value.tags,
            notes: value.notes,
            handoff_capsules: value.handoff_capsules,
            workflow_id: value.workflow_id,
            workflow_etag: value.workflow_etag,
            created_at: value.created_at,
        }
    }
}

impl From<EdgeRecord> for EdgeView {
    fn from(value: EdgeRecord) -> Self {
        Self {
            src: value.src,
            kind: value.kind,
            dst: value.dst,
        }
    }
}

#[derive(Debug)]
pub enum AppError {
    Io(std::io::Error),
    Db(rusqlite::Error),
    Event(EventWriteError),
    Import(ImportError),
    Sync(SyncError),
    Lock(LockError),
    RemoteInit(RemoteInitError),
    Fsck(FsckError),
    Doctor(DoctorError),
    Snapshot(SnapshotError),
    Perf(PerfError),
    Workflow(WorkflowError),
    ParseState(ParseKnotStateError),
    InvalidTransition(InvalidStateTransition),
    StaleWorkflowHead { expected: String, current: String },
    InvalidArgument(String),
    NotFound(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Io(err) => write!(f, "I/O error: {}", err),
            AppError::Db(err) => write!(f, "database error: {}", err),
            AppError::Event(err) => write!(f, "event write error: {}", err),
            AppError::Import(err) => write!(f, "import error: {}", err),
            AppError::Sync(err) => write!(f, "sync error: {}", err),
            AppError::Lock(err) => write!(f, "lock error: {}", err),
            AppError::RemoteInit(err) => write!(f, "remote init error: {}", err),
            AppError::Fsck(err) => write!(f, "fsck error: {}", err),
            AppError::Doctor(err) => write!(f, "doctor error: {}", err),
            AppError::Snapshot(err) => write!(f, "snapshot error: {}", err),
            AppError::Perf(err) => write!(f, "perf error: {}", err),
            AppError::Workflow(err) => write!(f, "workflow error: {}", err),
            AppError::ParseState(err) => write!(f, "state parse error: {}", err),
            AppError::InvalidTransition(err) => write!(f, "{}", err),
            AppError::StaleWorkflowHead { expected, current } => write!(
                f,
                "stale workflow_etag: expected '{}', current '{}'",
                expected, current
            ),
            AppError::InvalidArgument(message) => write!(f, "{}", message),
            AppError::NotFound(id) => write!(f, "knot '{}' not found in local cache", id),
        }
    }
}

impl Error for AppError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            AppError::Io(err) => Some(err),
            AppError::Db(err) => Some(err),
            AppError::Event(err) => Some(err),
            AppError::Import(err) => Some(err),
            AppError::Sync(err) => Some(err),
            AppError::Lock(err) => Some(err),
            AppError::RemoteInit(err) => Some(err),
            AppError::Fsck(err) => Some(err),
            AppError::Doctor(err) => Some(err),
            AppError::Snapshot(err) => Some(err),
            AppError::Perf(err) => Some(err),
            AppError::Workflow(err) => Some(err),
            AppError::ParseState(err) => Some(err),
            AppError::InvalidTransition(err) => Some(err),
            AppError::StaleWorkflowHead { .. } => None,
            AppError::InvalidArgument(_) => None,
            AppError::NotFound(_) => None,
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        AppError::Io(value)
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(value: rusqlite::Error) -> Self {
        AppError::Db(value)
    }
}

impl From<EventWriteError> for AppError {
    fn from(value: EventWriteError) -> Self {
        AppError::Event(value)
    }
}

impl From<ImportError> for AppError {
    fn from(value: ImportError) -> Self {
        AppError::Import(value)
    }
}

impl From<SyncError> for AppError {
    fn from(value: SyncError) -> Self {
        AppError::Sync(value)
    }
}

impl From<LockError> for AppError {
    fn from(value: LockError) -> Self {
        AppError::Lock(value)
    }
}

impl From<RemoteInitError> for AppError {
    fn from(value: RemoteInitError) -> Self {
        AppError::RemoteInit(value)
    }
}

impl From<FsckError> for AppError {
    fn from(value: FsckError) -> Self {
        AppError::Fsck(value)
    }
}

impl From<DoctorError> for AppError {
    fn from(value: DoctorError) -> Self {
        AppError::Doctor(value)
    }
}

impl From<SnapshotError> for AppError {
    fn from(value: SnapshotError) -> Self {
        AppError::Snapshot(value)
    }
}

impl From<PerfError> for AppError {
    fn from(value: PerfError) -> Self {
        AppError::Perf(value)
    }
}

impl From<WorkflowError> for AppError {
    fn from(value: WorkflowError) -> Self {
        AppError::Workflow(value)
    }
}

impl From<ParseKnotStateError> for AppError {
    fn from(value: ParseKnotStateError) -> Self {
        AppError::ParseState(value)
    }
}

impl From<InvalidStateTransition> for AppError {
    fn from(value: InvalidStateTransition) -> Self {
        AppError::InvalidTransition(value)
    }
}

#[cfg(test)]
mod tests;
