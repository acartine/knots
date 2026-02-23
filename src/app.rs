use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use rusqlite::Connection;
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use crate::db::{self, EdgeDirection, EdgeRecord, KnotCacheRecord, UpsertKnotHot};
use crate::domain::metadata::{normalize_datetime, MetadataEntry, MetadataEntryInput};
use crate::domain::state::{InvalidStateTransition, KnotState, ParseKnotStateError};
use crate::events::{
    new_event_id, now_utc_rfc3339, EventRecord, EventWriteError, EventWriter, FullEvent,
    FullEventKind, IndexEvent, IndexEventKind,
};
use crate::imports::{ImportError, ImportService, ImportStatus, ImportSummary};
use crate::sync::{SyncError, SyncService, SyncSummary};

pub struct App {
    conn: Connection,
    writer: EventWriter,
    repo_root: PathBuf,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct KnotView {
    pub id: String,
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
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EdgeView {
    pub src: String,
    pub kind: String,
    pub dst: String,
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
        let writer = EventWriter::new(repo_root.clone());
        Ok(Self {
            conn,
            writer,
            repo_root,
        })
    }

    pub fn create_knot(
        &self,
        title: &str,
        body: Option<&str>,
        initial_state: &str,
    ) -> Result<KnotView, AppError> {
        let state = KnotState::from_str(initial_state)?;
        let knot_id = format!("K-{}", Uuid::now_v7());
        let occurred_at = now_utc_rfc3339();

        let full_event = FullEvent::with_identity(
            new_event_id(),
            occurred_at.clone(),
            knot_id.clone(),
            FullEventKind::KnotCreated.as_str(),
            json!({
                "title": title,
                "state": state.as_str(),
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
                "updated_at": occurred_at,
                "terminal": state.is_terminal(),
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
                workflow_etag: Some(&index_event_id),
                created_at: Some(&occurred_at),
            },
        )?;

        let record = db::get_knot_hot(&self.conn, &knot_id)?
            .ok_or_else(|| AppError::NotFound(knot_id.clone()))?;
        Ok(KnotView::from(record))
    }

    pub fn set_state(&self, id: &str, next_state: &str, force: bool) -> Result<KnotView, AppError> {
        let current =
            db::get_knot_hot(&self.conn, id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
        let current_state = KnotState::from_str(&current.state)?;
        let next = KnotState::from_str(next_state)?;
        current_state.validate_transition(next, force)?;

        let occurred_at = now_utc_rfc3339();
        let full_event = FullEvent::with_identity(
            new_event_id(),
            occurred_at.clone(),
            id.to_string(),
            FullEventKind::KnotStateSet.as_str(),
            json!({
                "from": current_state.as_str(),
                "to": next.as_str(),
                "force": force,
            }),
        );

        let index_event_id = new_event_id();
        let idx_event = IndexEvent::with_identity(
            index_event_id.clone(),
            occurred_at.clone(),
            IndexEventKind::KnotHead.as_str(),
            json!({
                "knot_id": id,
                "title": current.title,
                "state": next.as_str(),
                "updated_at": occurred_at,
                "terminal": next.is_terminal(),
            }),
        );

        self.writer.write(&EventRecord::full(full_event))?;
        self.writer.write(&EventRecord::index(idx_event))?;

        db::upsert_knot_hot(
            &self.conn,
            &UpsertKnotHot {
                id,
                title: &current.title,
                state: next.as_str(),
                updated_at: &occurred_at,
                body: current.body.as_deref(),
                description: current.description.as_deref(),
                priority: current.priority,
                knot_type: current.knot_type.as_deref(),
                tags: &current.tags,
                notes: &current.notes,
                handoff_capsules: &current.handoff_capsules,
                workflow_etag: Some(&index_event_id),
                created_at: current.created_at.as_deref(),
            },
        )?;

        let updated =
            db::get_knot_hot(&self.conn, id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
        Ok(KnotView::from(updated))
    }

    pub fn update_knot(&self, id: &str, patch: UpdateKnotPatch) -> Result<KnotView, AppError> {
        if !patch.has_changes() {
            return Err(AppError::InvalidArgument(
                "update requires at least one field change".to_string(),
            ));
        }

        let current =
            db::get_knot_hot(&self.conn, id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
        let mut title = current.title.clone();
        let mut state = current.state.clone();
        let mut description = current.description.clone();
        let mut body = current.body.clone();
        let mut priority = current.priority;
        let mut knot_type = current.knot_type.clone();
        let mut tags = current.tags.clone();
        let mut notes = current.notes.clone();
        let mut handoff_capsules = current.handoff_capsules.clone();
        let occurred_at = now_utc_rfc3339();
        let mut full_events = Vec::new();

        if let Some(next_state_raw) = patch.status.as_deref() {
            let current_state = KnotState::from_str(&state)?;
            let next_state = KnotState::from_str(next_state_raw)?;
            current_state.validate_transition(next_state, patch.force)?;
            if current_state != next_state {
                state = next_state.as_str().to_string();
                full_events.push(FullEvent::with_identity(
                    new_event_id(),
                    occurred_at.clone(),
                    id.to_string(),
                    FullEventKind::KnotStateSet.as_str(),
                    json!({
                        "from": current_state.as_str(),
                        "to": next_state.as_str(),
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
            return Ok(KnotView::from(current));
        }

        for event in full_events {
            self.writer.write(&EventRecord::full(event))?;
        }

        let terminal = KnotState::from_str(&state)
            .map(|value| value.is_terminal())
            .unwrap_or(false);
        let index_event_id = new_event_id();
        let idx_event = IndexEvent::with_identity(
            index_event_id.clone(),
            occurred_at.clone(),
            IndexEventKind::KnotHead.as_str(),
            json!({
                "knot_id": id,
                "title": &title,
                "state": &state,
                "updated_at": occurred_at,
                "terminal": terminal,
            }),
        );
        self.writer.write(&EventRecord::index(idx_event))?;

        db::upsert_knot_hot(
            &self.conn,
            &UpsertKnotHot {
                id,
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
                workflow_etag: Some(&index_event_id),
                created_at: current.created_at.as_deref(),
            },
        )?;

        let updated =
            db::get_knot_hot(&self.conn, id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
        Ok(KnotView::from(updated))
    }

    pub fn list_knots(&self) -> Result<Vec<KnotView>, AppError> {
        Ok(db::list_knot_hot(&self.conn)?
            .into_iter()
            .map(KnotView::from)
            .collect())
    }

    pub fn show_knot(&self, id: &str) -> Result<Option<KnotView>, AppError> {
        Ok(db::get_knot_hot(&self.conn, id)?.map(KnotView::from))
    }

    pub fn sync(&self) -> Result<SyncSummary, AppError> {
        let service = SyncService::new(&self.conn, self.repo_root.clone());
        Ok(service.sync()?)
    }

    pub fn add_edge(&self, src: &str, kind: &str, dst: &str) -> Result<EdgeView, AppError> {
        self.apply_edge_change(src, kind, dst, true)
    }

    pub fn remove_edge(&self, src: &str, kind: &str, dst: &str) -> Result<EdgeView, AppError> {
        self.apply_edge_change(src, kind, dst, false)
    }

    pub fn list_edges(&self, id: &str, direction: &str) -> Result<Vec<EdgeView>, AppError> {
        let direction = parse_edge_direction(direction)?;
        let rows = db::list_edges(&self.conn, id, direction)?;
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

        let terminal = KnotState::from_str(&current.state)
            .map(|state| state.is_terminal())
            .unwrap_or(false);
        let index_event_id = new_event_id();
        let idx_event = IndexEvent::with_identity(
            index_event_id.clone(),
            occurred_at.clone(),
            IndexEventKind::KnotHead.as_str(),
            json!({
                "knot_id": src,
                "title": current.title,
                "state": current.state,
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

impl From<KnotCacheRecord> for KnotView {
    fn from(value: KnotCacheRecord) -> Self {
        Self {
            id: value.id,
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
    ParseState(ParseKnotStateError),
    InvalidTransition(InvalidStateTransition),
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
            AppError::ParseState(err) => write!(f, "state parse error: {}", err),
            AppError::InvalidTransition(err) => write!(f, "{}", err),
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
            AppError::ParseState(err) => Some(err),
            AppError::InvalidTransition(err) => Some(err),
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
