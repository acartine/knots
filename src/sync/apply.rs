use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use time::OffsetDateTime;

use crate::db::{self, UpsertKnotHot};
use crate::domain::metadata::MetadataEntry;
use crate::events::{FullEvent, IndexEvent, IndexEventKind, WorkflowPrecondition};
use crate::snapshots::apply_latest_snapshots;
use crate::tiering::{classify_knot_tier, CacheTier};

use super::{GitAdapter, SyncError, SyncSummary};

pub struct IncrementalApplier<'a> {
    conn: &'a Connection,
    worktree: PathBuf,
    git: GitAdapter,
}

impl<'a> IncrementalApplier<'a> {
    pub fn new(conn: &'a Connection, worktree: PathBuf, git: GitAdapter) -> Self {
        Self {
            conn,
            worktree,
            git,
        }
    }

    pub fn apply_to_head(&mut self, target_head: &str) -> Result<SyncSummary, SyncError> {
        let bootstrap = db::get_meta(self.conn, "last_index_head_commit")?.is_none()
            && db::get_meta(self.conn, "last_full_head_commit")?.is_none();
        if bootstrap {
            let _ = apply_latest_snapshots(self.conn, &self.worktree).map_err(|err| {
                SyncError::SnapshotLoad {
                    message: err.to_string(),
                }
            })?;
        }

        let index_files =
            self.changed_files("last_index_head_commit", ".knots/index", target_head)?;
        let full_files =
            self.changed_files("last_full_head_commit", ".knots/events", target_head)?;

        let mut summary = SyncSummary {
            target_head: target_head.to_string(),
            index_files: index_files.len() as u64,
            full_files: full_files.len() as u64,
            knot_updates: 0,
            edge_adds: 0,
            edge_removes: 0,
        };

        for rel_path in index_files {
            if self.apply_index_event(&rel_path)? {
                summary.knot_updates += 1;
            }
        }

        for rel_path in full_files {
            match self.apply_full_event(&rel_path)? {
                FullApplyOutcome::EdgeAdded => summary.edge_adds += 1,
                FullApplyOutcome::EdgeRemoved => summary.edge_removes += 1,
                FullApplyOutcome::Ignored => {}
            }
        }

        db::set_meta(self.conn, "last_index_head_commit", target_head)?;
        db::set_meta(self.conn, "last_full_head_commit", target_head)?;
        db::set_meta(self.conn, "sync_pending", "false")?;
        Ok(summary)
    }

    fn changed_files(
        &self,
        meta_key: &str,
        prefix: &str,
        target_head: &str,
    ) -> Result<Vec<PathBuf>, SyncError> {
        let base = db::get_meta(self.conn, meta_key)?;
        if let Some(base_head) = base {
            if base_head == target_head {
                return Ok(Vec::new());
            }

            match self
                .git
                .diff_name_only(&self.worktree, &base_head, target_head, prefix)
            {
                Ok(mut files) => {
                    files.retain(|path| path.extension().is_some_and(|ext| ext == "json"));
                    files.sort();
                    return Ok(files);
                }
                Err(err) if err.is_unknown_revision() => {}
                Err(err) => return Err(err),
            }
        }

        let mut files = self.scan_json_files(prefix)?;
        files.sort();
        Ok(files)
    }

    fn scan_json_files(&self, prefix: &str) -> Result<Vec<PathBuf>, SyncError> {
        let root = self.worktree.join(prefix);
        if !root.exists() {
            return Ok(Vec::new());
        }

        let mut stack = vec![root];
        let mut files = Vec::new();
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir)? {
                let path = entry?.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|ext| ext != "json") {
                    continue;
                }
                let relative = path
                    .strip_prefix(&self.worktree)
                    .map_err(|err| SyncError::InvalidEvent {
                        path: path.clone(),
                        message: format!("failed to relativize path: {}", err),
                    })?
                    .to_path_buf();
                files.push(relative);
            }
        }
        Ok(files)
    }

    fn apply_index_event(&self, relative_path: &Path) -> Result<bool, SyncError> {
        let absolute_path = self.worktree.join(relative_path);
        if !absolute_path.exists() {
            return Ok(false);
        }

        let event: IndexEvent = read_json_file(&absolute_path)?;
        if event.event_type != IndexEventKind::KnotHead.as_str() {
            return Ok(false);
        }

        let data = event
            .data
            .as_object()
            .ok_or_else(|| invalid_event(&absolute_path, "idx.knot_head data must be an object"))?;

        let knot_id = required_string(data, "knot_id", &absolute_path)?;
        let title = required_string(data, "title", &absolute_path)?;
        let state = required_string(data, "state", &absolute_path)?;
        let updated_at = required_string(data, "updated_at", &absolute_path)?;
        let workflow_id =
            required_string(data, "workflow_id", &absolute_path)?.to_ascii_lowercase();

        if is_stale_precondition(self.conn, &knot_id, event.precondition.as_ref())? {
            return Ok(false);
        }

        let hot_window_days = db::get_hot_window_days(self.conn)?;
        let terminal_flag = data
            .get("terminal")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let now = OffsetDateTime::now_utc();
        let tier = if terminal_flag {
            CacheTier::Cold
        } else {
            classify_knot_tier(&state, &updated_at, hot_window_days, now)
        };

        if tier == CacheTier::Cold {
            db::delete_knot_hot(self.conn, &knot_id)?;
            db::delete_knot_warm(self.conn, &knot_id)?;
            db::upsert_cold_catalog(self.conn, &knot_id, &title, &state, &updated_at)?;
            return Ok(true);
        }

        let existing = db::get_knot_hot(self.conn, &knot_id)?;
        let body = existing.as_ref().and_then(|record| record.body.clone());
        let description = existing
            .as_ref()
            .and_then(|record| record.description.clone());
        let priority = existing.as_ref().and_then(|record| record.priority);
        let knot_type = existing
            .as_ref()
            .and_then(|record| record.knot_type.clone());
        let tags = existing
            .as_ref()
            .map(|record| record.tags.clone())
            .unwrap_or_default();
        let notes = existing
            .as_ref()
            .map(|record| record.notes.clone())
            .unwrap_or_default();
        let handoff_capsules = existing
            .as_ref()
            .map(|record| record.handoff_capsules.clone())
            .unwrap_or_default();
        let created_at = existing
            .as_ref()
            .and_then(|record| record.created_at.clone())
            .unwrap_or_else(|| updated_at.clone());

        match tier {
            CacheTier::Hot => {
                db::upsert_knot_hot(
                    self.conn,
                    &UpsertKnotHot {
                        id: &knot_id,
                        title: &title,
                        state: &state,
                        updated_at: &updated_at,
                        body: body.as_deref(),
                        description: description.as_deref(),
                        priority,
                        knot_type: knot_type.as_deref(),
                        tags: &tags,
                        notes: &notes,
                        handoff_capsules: &handoff_capsules,
                        workflow_id: &workflow_id,
                        workflow_etag: Some(&event.event_id),
                        created_at: Some(&created_at),
                    },
                )?;
                db::delete_cold_catalog(self.conn, &knot_id)?;
            }
            CacheTier::Warm => {
                db::delete_knot_hot(self.conn, &knot_id)?;
                db::upsert_knot_warm(self.conn, &knot_id, &title)?;
                db::delete_cold_catalog(self.conn, &knot_id)?;
            }
            CacheTier::Cold => {}
        }
        Ok(true)
    }

    fn apply_full_event(&self, relative_path: &Path) -> Result<FullApplyOutcome, SyncError> {
        let absolute_path = self.worktree.join(relative_path);
        if !absolute_path.exists() {
            return Ok(FullApplyOutcome::Ignored);
        }

        let event: FullEvent = read_json_file(&absolute_path)?;
        let data = event
            .data
            .as_object()
            .ok_or_else(|| invalid_event(&absolute_path, "full event data must be an object"))?;

        if is_stale_precondition(self.conn, &event.knot_id, event.precondition.as_ref())? {
            return Ok(FullApplyOutcome::Ignored);
        }

        match event.event_type.as_str() {
            "knot.edge_add" => {
                let kind = required_string(data, "kind", &absolute_path)?;
                let dst = required_string(data, "dst", &absolute_path)?;
                db::insert_edge(self.conn, &event.knot_id, &kind, &dst)?;
                Ok(FullApplyOutcome::EdgeAdded)
            }
            "knot.edge_remove" => {
                let kind = required_string(data, "kind", &absolute_path)?;
                let dst = required_string(data, "dst", &absolute_path)?;
                db::delete_edge(self.conn, &event.knot_id, &kind, &dst)?;
                Ok(FullApplyOutcome::EdgeRemoved)
            }
            "knot.description_set" => {
                self.apply_metadata_update(&event.knot_id, |record| {
                    record.description = optional_string(data.get("description"));
                    record.body = record.description.clone();
                })?;
                Ok(FullApplyOutcome::Ignored)
            }
            "knot.priority_set" => {
                self.apply_metadata_update(&event.knot_id, |record| {
                    record.priority = optional_i64(data.get("priority"));
                })?;
                Ok(FullApplyOutcome::Ignored)
            }
            "knot.type_set" => {
                self.apply_metadata_update(&event.knot_id, |record| {
                    record.knot_type = optional_string(data.get("type"));
                })?;
                Ok(FullApplyOutcome::Ignored)
            }
            "knot.tag_add" => {
                let tag = required_string(data, "tag", &absolute_path)?
                    .trim()
                    .to_ascii_lowercase();
                if !tag.is_empty() {
                    self.apply_metadata_update(&event.knot_id, |record| {
                        if !record.tags.iter().any(|existing| existing == &tag) {
                            record.tags.push(tag.clone());
                        }
                    })?;
                }
                Ok(FullApplyOutcome::Ignored)
            }
            "knot.tag_remove" => {
                let tag = required_string(data, "tag", &absolute_path)?
                    .trim()
                    .to_ascii_lowercase();
                if !tag.is_empty() {
                    self.apply_metadata_update(&event.knot_id, |record| {
                        record.tags.retain(|existing| existing != &tag);
                    })?;
                }
                Ok(FullApplyOutcome::Ignored)
            }
            "knot.note_added" => {
                let entry = parse_metadata_entry(data, &absolute_path)?;
                self.apply_metadata_update(&event.knot_id, |record| {
                    if !record
                        .notes
                        .iter()
                        .any(|existing| existing.entry_id == entry.entry_id)
                    {
                        record.notes.push(entry.clone());
                    }
                })?;
                Ok(FullApplyOutcome::Ignored)
            }
            "knot.handoff_capsule_added" => {
                let entry = parse_metadata_entry(data, &absolute_path)?;
                self.apply_metadata_update(&event.knot_id, |record| {
                    if !record
                        .handoff_capsules
                        .iter()
                        .any(|existing| existing.entry_id == entry.entry_id)
                    {
                        record.handoff_capsules.push(entry.clone());
                    }
                })?;
                Ok(FullApplyOutcome::Ignored)
            }
            _ => Ok(FullApplyOutcome::Ignored),
        }
    }

    fn apply_metadata_update<F>(&self, knot_id: &str, mutate: F) -> Result<(), SyncError>
    where
        F: FnOnce(&mut MetadataProjection),
    {
        let Some(existing) = db::get_knot_hot(self.conn, knot_id)? else {
            return Ok(());
        };

        let mut projection = MetadataProjection {
            title: existing.title,
            state: existing.state,
            updated_at: existing.updated_at,
            body: existing.body,
            description: existing.description,
            priority: existing.priority,
            knot_type: existing.knot_type,
            tags: existing.tags,
            notes: existing.notes,
            handoff_capsules: existing.handoff_capsules,
            workflow_id: existing.workflow_id,
            workflow_etag: existing.workflow_etag,
            created_at: existing.created_at,
        };
        mutate(&mut projection);

        db::upsert_knot_hot(
            self.conn,
            &UpsertKnotHot {
                id: knot_id,
                title: &projection.title,
                state: &projection.state,
                updated_at: &projection.updated_at,
                body: projection.body.as_deref(),
                description: projection.description.as_deref(),
                priority: projection.priority,
                knot_type: projection.knot_type.as_deref(),
                tags: &projection.tags,
                notes: &projection.notes,
                handoff_capsules: &projection.handoff_capsules,
                workflow_id: &projection.workflow_id,
                workflow_etag: projection.workflow_etag.as_deref(),
                created_at: projection.created_at.as_deref(),
            },
        )?;
        Ok(())
    }
}

enum FullApplyOutcome {
    EdgeAdded,
    EdgeRemoved,
    Ignored,
}

struct MetadataProjection {
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

fn read_json_file<T>(path: &Path) -> Result<T, SyncError>
where
    T: DeserializeOwned,
{
    let bytes = std::fs::read(path)?;
    serde_json::from_slice(&bytes)
        .map_err(|err| invalid_event(path, &format!("invalid JSON payload: {}", err)))
}

fn required_string(
    object: &Map<String, Value>,
    key: &str,
    path: &Path,
) -> Result<String, SyncError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(|value| value.to_string())
        .ok_or_else(|| invalid_event(path, &format!("missing '{}' string field", key)))
}

fn optional_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .and_then(|raw| {
            if raw.is_empty() {
                None
            } else {
                Some(raw.to_string())
            }
        })
}

fn optional_i64(value: Option<&Value>) -> Option<i64> {
    value.and_then(Value::as_i64)
}

fn parse_metadata_entry(
    object: &Map<String, Value>,
    path: &Path,
) -> Result<MetadataEntry, SyncError> {
    let entry_id = required_string(object, "entry_id", path)?;
    let content = required_string(object, "content", path)?;
    let username = required_string(object, "username", path)?;
    let datetime = required_string(object, "datetime", path)?;
    let agentname = required_string(object, "agentname", path)?;
    let model = required_string(object, "model", path)?;
    let version = required_string(object, "version", path)?;
    Ok(MetadataEntry {
        entry_id,
        content,
        username,
        datetime,
        agentname,
        model,
        version,
    })
}

fn invalid_event(path: &Path, message: &str) -> SyncError {
    SyncError::InvalidEvent {
        path: path.to_path_buf(),
        message: message.to_string(),
    }
}

fn is_stale_precondition(
    conn: &Connection,
    knot_id: &str,
    precondition: Option<&WorkflowPrecondition>,
) -> Result<bool, SyncError> {
    let Some(precondition) = precondition else {
        return Ok(false);
    };
    let current = db::get_knot_hot(conn, knot_id)?
        .and_then(|record| record.workflow_etag)
        .unwrap_or_default();
    Ok(current != precondition.workflow_etag)
}

#[cfg(test)]
#[path = "apply_tests_ext.rs"]
mod tests_ext;
