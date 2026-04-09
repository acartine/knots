use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::Value;
use time::OffsetDateTime;

use crate::db;
use crate::events::{FullEvent, IndexEvent, IndexEventKind};
use crate::snapshots::apply_latest_snapshots;
use crate::tiering::{classify_knot_tier, CacheTier};

use super::{GitAdapter, SyncError, SyncSummary};

#[path = "apply_helpers.rs"]
mod apply_helpers;
use apply_helpers::{
    build_index_upsert, current_unix_ms_string, invalid_event, is_stale_precondition, optional_i64,
    optional_string, parse_gate_data, parse_invariants, parse_lease_data, parse_metadata_entry,
    read_json_file, required_profile_id, required_string, required_workflow_id, IndexUpsertParams,
    MetadataProjection,
};

pub struct IncrementalApplier<'a> {
    conn: &'a Connection,
    worktree: PathBuf,
    git: GitAdapter,
    known_workflows: HashSet<String>,
    warned_legacy: HashSet<String>,
}

impl<'a> IncrementalApplier<'a> {
    pub fn new(
        conn: &'a Connection,
        worktree: PathBuf,
        git: GitAdapter,
        known_workflows: HashSet<String>,
    ) -> Self {
        Self {
            conn,
            worktree,
            git,
            known_workflows,
            warned_legacy: HashSet::new(),
        }
    }

    #[cfg(test)]
    pub fn new_with_builtins(conn: &'a Connection, worktree: PathBuf, git: GitAdapter) -> Self {
        let known_workflows = crate::domain::knot_type::KnotType::ALL
            .into_iter()
            .map(crate::installed_workflows::builtin_workflow_id_for_knot_type)
            .collect();
        Self::new(conn, worktree, git, known_workflows)
    }

    pub fn apply_to_head(&mut self, target_head: &str) -> Result<SyncSummary, SyncError> {
        let bootstrap = db::get_meta(self.conn, "last_index_head_commit")?.is_none()
            && db::get_meta(self.conn, "last_full_head_commit")?.is_none();
        if bootstrap {
            let _ = crate::trace::measure("apply_snapshots", || {
                apply_latest_snapshots(self.conn, &self.worktree).map_err(|err| {
                    SyncError::SnapshotLoad {
                        message: err.to_string(),
                    }
                })
            })?;
        }

        let index_files = crate::trace::measure("changed_index_files", || {
            self.changed_files("last_index_head_commit", ".knots/index", target_head)
        })?;
        let full_files = crate::trace::measure("changed_event_files", || {
            self.changed_files("last_full_head_commit", ".knots/events", target_head)
        })?;

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
        db::set_meta(
            self.conn,
            "last_sync_success_at_ms",
            &current_unix_ms_string(),
        )?;
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

    fn apply_index_event(&mut self, relative_path: &Path) -> Result<bool, SyncError> {
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
        let profile_id = required_profile_id(data, &absolute_path)?.to_ascii_lowercase();
        let resolved = required_workflow_id(data, &absolute_path)?;
        if let Some(ref legacy_id) = resolved.converted_from {
            if self.warned_legacy.insert(legacy_id.clone()) {
                eprintln!(
                    "warning: converted legacy workflow '{}' to '{}'; \
                     this is probably fine but you should know",
                    legacy_id, resolved.id
                );
            }
        }
        if !self.known_workflows.contains(&resolved.id) {
            return Err(invalid_event(
                &absolute_path,
                &format!(
                    "unrecognized workflow '{}'; \
                     upgrade knots with `kno upgrade`",
                    resolved.id
                ),
            ));
        }
        let workflow_id = resolved.id;

        if is_stale_precondition(self.conn, &knot_id, event.precondition.as_ref())? {
            return Ok(false);
        }

        let tier = resolve_tier(self.conn, data, &state, &updated_at)?;

        if tier == CacheTier::Cold {
            db::delete_knot_hot(self.conn, &knot_id)?;
            db::delete_knot_warm(self.conn, &knot_id)?;
            db::upsert_cold_catalog(self.conn, &knot_id, &title, &state, &updated_at)?;
            return Ok(true);
        }

        let upsert = build_index_upsert(&IndexUpsertParams {
            conn: self.conn,
            data,
            absolute_path: &absolute_path,
            knot_id: &knot_id,
            title: &title,
            state: &state,
            updated_at: &updated_at,
            profile_id: &profile_id,
            workflow_id: &workflow_id,
            event_id: &event.event_id,
        })?;

        match tier {
            CacheTier::Hot => {
                upsert.upsert(self.conn, &knot_id)?;
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
            t => {
                self.apply_metadata_event(t, data, &event.knot_id, &absolute_path)?;
                Ok(FullApplyOutcome::Ignored)
            }
        }
    }

    fn apply_metadata_event(
        &self,
        event_type: &str,
        data: &serde_json::Map<String, Value>,
        knot_id: &str,
        path: &Path,
    ) -> Result<(), SyncError> {
        match event_type {
            "knot.description_set" => self.apply_metadata_update(knot_id, |r| {
                r.description = optional_string(data.get("description"));
                r.body = r.description.clone();
            }),
            "knot.acceptance_set" => self.apply_metadata_update(knot_id, |r| {
                r.acceptance = optional_string(data.get("acceptance"));
            }),
            "knot.priority_set" => self.apply_metadata_update(knot_id, |r| {
                r.priority = optional_i64(data.get("priority"));
            }),
            "knot.type_set" => self.apply_metadata_update(knot_id, |r| {
                r.knot_type = optional_string(data.get("type"));
            }),
            "knot.tag_add" => self.apply_tag_add(data, knot_id, path),
            "knot.tag_remove" => self.apply_tag_remove(data, knot_id, path),
            "knot.note_added" => {
                let entry = parse_metadata_entry(data, path)?;
                self.apply_metadata_update(knot_id, |r| {
                    if !r.notes.iter().any(|e| e.entry_id == entry.entry_id) {
                        r.notes.push(entry.clone());
                    }
                })
            }
            "knot.handoff_capsule_added" => {
                let entry = parse_metadata_entry(data, path)?;
                self.apply_metadata_update(knot_id, |r| {
                    if !r
                        .handoff_capsules
                        .iter()
                        .any(|e| e.entry_id == entry.entry_id)
                    {
                        r.handoff_capsules.push(entry.clone());
                    }
                })
            }
            "knot.invariants_set" => {
                let invariants = parse_invariants(data, path)?;
                self.apply_metadata_update(knot_id, |r| r.invariants = invariants)
            }
            "knot.gate_data_set" => {
                let gate_data = parse_gate_data(data, path)?;
                self.apply_metadata_update(knot_id, |r| r.gate_data = gate_data)
            }
            "knot.lease_data_set" => {
                let ld = parse_lease_data(data, path)?;
                self.apply_metadata_update(knot_id, |r| r.lease_data = ld)
            }
            "knot.lease_id_set" => {
                let lid = optional_string(data.get("lease_id"));
                self.apply_metadata_update(knot_id, |r| r.lease_id = lid)
            }
            _ => Ok(()),
        }
    }

    fn apply_tag_add(
        &self,
        data: &serde_json::Map<String, Value>,
        knot_id: &str,
        path: &Path,
    ) -> Result<(), SyncError> {
        let tag = required_string(data, "tag", path)?
            .trim()
            .to_ascii_lowercase();
        if !tag.is_empty() {
            self.apply_metadata_update(knot_id, |r| {
                if !r.tags.iter().any(|existing| existing == &tag) {
                    r.tags.push(tag.clone());
                }
            })?;
        }
        Ok(())
    }

    fn apply_tag_remove(
        &self,
        data: &serde_json::Map<String, Value>,
        knot_id: &str,
        path: &Path,
    ) -> Result<(), SyncError> {
        let tag = required_string(data, "tag", path)?
            .trim()
            .to_ascii_lowercase();
        if !tag.is_empty() {
            self.apply_metadata_update(knot_id, |r| {
                r.tags.retain(|existing| existing != &tag);
            })?;
        }
        Ok(())
    }

    fn apply_metadata_update<F>(&self, knot_id: &str, mutate: F) -> Result<(), SyncError>
    where
        F: FnOnce(&mut MetadataProjection),
    {
        let Some(existing) = db::get_knot_hot(self.conn, knot_id)? else {
            return Ok(());
        };

        let mut projection = MetadataProjection::from_existing(&existing);
        mutate(&mut projection);
        projection.upsert(self.conn, knot_id)?;
        Ok(())
    }
}

enum FullApplyOutcome {
    EdgeAdded,
    EdgeRemoved,
    Ignored,
}

fn resolve_tier(
    conn: &Connection,
    data: &serde_json::Map<String, Value>,
    state: &str,
    updated_at: &str,
) -> Result<CacheTier, SyncError> {
    let hot_window_days = db::get_hot_window_days(conn)?;
    let terminal_flag = data
        .get("terminal")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let now = OffsetDateTime::now_utc();
    if terminal_flag {
        Ok(CacheTier::Cold)
    } else {
        Ok(classify_knot_tier(state, updated_at, hot_window_days, now))
    }
}

#[cfg(test)]
#[path = "apply_tests_acceptance_ext.rs"]
mod tests_acceptance_ext;
#[cfg(test)]
#[path = "apply_tests_event_paths.rs"]
mod tests_event_paths;
#[cfg(test)]
#[path = "apply_tests_ext.rs"]
mod tests_ext;
#[cfg(test)]
#[path = "apply_tests_invariant.rs"]
mod tests_invariant;
