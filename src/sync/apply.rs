use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::db::{self, UpsertKnotHot};
use crate::events::{FullEvent, IndexEvent, IndexEventKind};

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
                if !path.extension().is_some_and(|ext| ext == "json") {
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

        let is_terminal = data
            .get("terminal")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if is_terminal {
            db::delete_knot_hot(self.conn, &knot_id)?;
            db::delete_knot_warm(self.conn, &knot_id)?;
            return Ok(true);
        }

        let existing = db::get_knot_hot(self.conn, &knot_id)?;
        let body = existing.as_ref().and_then(|record| record.body.clone());
        let created_at = existing
            .as_ref()
            .and_then(|record| record.created_at.clone())
            .unwrap_or_else(|| updated_at.clone());

        db::upsert_knot_hot(
            self.conn,
            &UpsertKnotHot {
                id: &knot_id,
                title: &title,
                state: &state,
                updated_at: &updated_at,
                body: body.as_deref(),
                workflow_etag: Some(&event.event_id),
                created_at: Some(&created_at),
            },
        )?;
        Ok(true)
    }

    fn apply_full_event(&self, relative_path: &Path) -> Result<FullApplyOutcome, SyncError> {
        let absolute_path = self.worktree.join(relative_path);
        if !absolute_path.exists() {
            return Ok(FullApplyOutcome::Ignored);
        }

        let event: FullEvent = read_json_file(&absolute_path)?;
        match event.event_type.as_str() {
            "knot.edge_add" => {
                let data = event.data.as_object().ok_or_else(|| {
                    invalid_event(&absolute_path, "knot.edge_add data must be an object")
                })?;
                let kind = required_string(data, "kind", &absolute_path)?;
                let dst = required_string(data, "dst", &absolute_path)?;
                db::insert_edge(self.conn, &event.knot_id, &kind, &dst)?;
                Ok(FullApplyOutcome::EdgeAdded)
            }
            "knot.edge_remove" => {
                let data = event.data.as_object().ok_or_else(|| {
                    invalid_event(&absolute_path, "knot.edge_remove data must be an object")
                })?;
                let kind = required_string(data, "kind", &absolute_path)?;
                let dst = required_string(data, "dst", &absolute_path)?;
                db::delete_edge(self.conn, &event.knot_id, &kind, &dst)?;
                Ok(FullApplyOutcome::EdgeRemoved)
            }
            _ => Ok(FullApplyOutcome::Ignored),
        }
    }
}

enum FullApplyOutcome {
    EdgeAdded,
    EdgeRemoved,
    Ignored,
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

fn invalid_event(path: &Path, message: &str) -> SyncError {
    SyncError::InvalidEvent {
        path: path.to_path_buf(),
        message: message.to_string(),
    }
}
