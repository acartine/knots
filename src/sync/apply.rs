use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::Value;

use crate::db;
use crate::events::{FullEvent, IndexEvent, IndexEventKind};
use crate::snapshots::apply_latest_snapshots;
use crate::tiering::CacheTier;

use super::{GitAdapter, SyncError, SyncSummary};

#[path = "apply_helpers.rs"]
mod apply_helpers;
#[path = "apply_projection.rs"]
mod apply_projection;
#[path = "apply_quarantine.rs"]
mod apply_quarantine;
#[path = "apply_state.rs"]
mod apply_state;
#[path = "apply_step_history.rs"]
mod apply_step_history;
use apply_helpers::{
    build_index_upsert, current_unix_ms_string, invalid_event, is_stale_full_precondition,
    is_stale_precondition, optional_i64, optional_string, parse_execution_plan_data,
    parse_gate_data, parse_invariants, parse_lease_data, parse_metadata_entry, parse_scope_data,
    parse_string_vec, read_json_file, required_profile_id, required_string, required_workflow_id,
    IndexUpsertParams, WorkflowIdResolution,
};
use apply_projection::MetadataProjection;
use apply_state::resolve_tier;
use apply_state::FullApplyOutcome;
use apply_step_history::apply_state_set_step_history;

pub struct IncrementalApplier<'a> {
    conn: &'a Connection,
    worktree: PathBuf,
    git: GitAdapter,
    known_workflows: HashSet<String>,
    warned_legacy: HashSet<String>,
    skipped_unknown_workflow_knots: HashSet<String>,
    /// Knots this machine currently holds a local lease on (including the
    /// lease knots themselves). Their events are quarantined instead of
    /// applied -- see `apply_quarantine`.
    locally_leased_knot_ids: HashSet<String>,
    /// Watermark captured at the start of the current pass, used as the
    /// `base_commit` for any knot newly quarantined this pass.
    pre_pass_base_commit: String,
}

impl<'a> IncrementalApplier<'a> {
    pub fn new(
        conn: &'a Connection,
        worktree: PathBuf,
        git: GitAdapter,
        known_workflows: HashSet<String>,
        locally_leased_knot_ids: HashSet<String>,
    ) -> Self {
        Self {
            conn,
            worktree,
            git,
            known_workflows,
            warned_legacy: HashSet::new(),
            skipped_unknown_workflow_knots: HashSet::new(),
            locally_leased_knot_ids,
            pre_pass_base_commit: String::new(),
        }
    }

    #[cfg(test)]
    pub fn new_with_builtins(conn: &'a Connection, worktree: PathBuf, git: GitAdapter) -> Self {
        let known_workflows = crate::domain::knot_type::KnotType::ALL
            .into_iter()
            .map(crate::installed_workflows::builtin_workflow_id_for_knot_type)
            .collect();
        Self::new(conn, worktree, git, known_workflows, HashSet::new())
    }

    /// Test-only constructor for exercising the per-knot quarantine filter.
    #[cfg(test)]
    pub fn new_with_builtins_and_leased(
        conn: &'a Connection,
        worktree: PathBuf,
        git: GitAdapter,
        locally_leased_knot_ids: HashSet<String>,
    ) -> Self {
        let known_workflows = crate::domain::knot_type::KnotType::ALL
            .into_iter()
            .map(crate::installed_workflows::builtin_workflow_id_for_knot_type)
            .collect();
        Self::new(
            conn,
            worktree,
            git,
            known_workflows,
            locally_leased_knot_ids,
        )
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

        self.pre_pass_base_commit = self.resolve_pre_pass_base_commit()?;
        let drained =
            crate::trace::measure("drain_quarantine", || self.drain_quarantine(target_head))?;

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
            held_back_knots: Vec::new(),
        };

        for rel_path in index_files {
            if drained.contains(&rel_path) {
                continue;
            }
            if self.apply_index_event(&rel_path)? {
                summary.knot_updates += 1;
            }
        }

        for rel_path in full_files {
            if drained.contains(&rel_path) {
                continue;
            }
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
        summary.held_back_knots = db::list_quarantined_knots(self.conn)?
            .into_iter()
            .map(|row| row.knot_id)
            .collect();
        Ok(summary)
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
        if self.locally_leased_knot_ids.contains(&knot_id) {
            db::quarantine_knot_if_absent(self.conn, &knot_id, &self.pre_pass_base_commit)?;
            return Ok(false);
        }
        let title = required_string(data, "title", &absolute_path)?;
        let state = required_string(data, "state", &absolute_path)?;
        let updated_at = required_string(data, "updated_at", &absolute_path)?;
        let profile_id = required_profile_id(data, &absolute_path)?.to_ascii_lowercase();
        let resolved = required_workflow_id(data, &absolute_path)?;
        match &resolved.resolution {
            WorkflowIdResolution::Direct => {}
            WorkflowIdResolution::ConvertedLegacy(legacy_id) => {
                if self.warned_legacy.insert(legacy_id.clone()) {
                    eprintln!(
                        "warning: converted legacy workflow '{}' to '{}'; \
                         this is probably fine but you should know",
                        legacy_id, resolved.id
                    );
                }
            }
            WorkflowIdResolution::InferredFromType(knot_type) => {
                let key = format!("<unspecified:{knot_type}>");
                if self.warned_legacy.insert(key) {
                    eprintln!(
                        "warning: inferred workflow '{}' from knot type '{}' for \
                         events missing workflow_id; this is probably fine but you should know",
                        resolved.id, knot_type
                    );
                }
            }
        }
        if !self.known_workflows.contains(&resolved.id) {
            self.skipped_unknown_workflow_knots.insert(knot_id.clone());
            eprintln!(
                "warning: can't import knot '{}', unknown workflow '{}'. \
                 The knot creator should install the workflow into the repository \
                 so other users can view the knot.",
                knot_id, resolved.id
            );
            return Ok(false);
        }
        let workflow_id = resolved.id;
        self.skipped_unknown_workflow_knots.remove(&knot_id);

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
        if self.locally_leased_knot_ids.contains(&event.knot_id) {
            db::quarantine_knot_if_absent(self.conn, &event.knot_id, &self.pre_pass_base_commit)?;
            return Ok(FullApplyOutcome::Ignored);
        }
        if self.skipped_unknown_workflow_knots.contains(&event.knot_id) {
            return Ok(FullApplyOutcome::Ignored);
        }
        let data = event
            .data
            .as_object()
            .ok_or_else(|| invalid_event(&absolute_path, "full event data must be an object"))?;

        if is_stale_full_precondition(self.conn, &event)? {
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
                self.apply_metadata_event(
                    t,
                    data,
                    &event.knot_id,
                    &event.occurred_at,
                    &absolute_path,
                )?;
                Ok(FullApplyOutcome::Ignored)
            }
        }
    }

    fn apply_metadata_event(
        &self,
        event_type: &str,
        data: &serde_json::Map<String, Value>,
        knot_id: &str,
        occurred_at: &str,
        path: &Path,
    ) -> Result<(), SyncError> {
        match event_type {
            // Compat: pre-fix `knot.created` events carried the description
            // inline as `body`. Newer creates emit a separate
            // `knot.description_set`, but old events still need to populate
            // description on first apply. Tracked by knot `83b1` for removal
            // once that cohort ages out.
            "knot.created" => {
                let body = optional_string(data.get("body"));
                self.apply_metadata_update(knot_id, |r| {
                    if r.description.is_none() {
                        r.description = body.clone();
                        r.body = body.clone();
                    }
                })
            }
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
            "knot.verification_steps_set" => {
                let steps = parse_string_vec(data, path, "verification_steps")?;
                self.apply_metadata_update(knot_id, |r| r.verification_steps = steps)
            }
            "knot.gate_data_set" => {
                let gate_data = parse_gate_data(data, path)?;
                self.apply_metadata_update(knot_id, |r| r.gate_data = gate_data)
            }
            "knot.lease_data_set" => {
                let ld = parse_lease_data(data, path)?;
                self.apply_metadata_update(knot_id, |r| r.lease_data = ld)
            }
            "knot.execution_plan_data_set" => {
                let execution_plan_data = parse_execution_plan_data(data, path)?;
                self.apply_metadata_update(knot_id, |r| r.execution_plan_data = execution_plan_data)
            }
            "knot.scope_set" => {
                let scope_data = parse_scope_data(data, path)?;
                self.apply_metadata_update(knot_id, |r| r.scope_data = scope_data)
            }
            "knot.lease_id_set" => {
                let lid = optional_string(data.get("lease_id"));
                self.apply_metadata_update(knot_id, |r| r.lease_id = lid)
            }
            "knot.state_set" => self.apply_metadata_update(knot_id, |r| {
                apply_state_set_step_history(r, data, occurred_at);
            }),
            _ => Ok(()),
        }
    }

    fn apply_tag_add(
        &self,
        data: &serde_json::Map<String, Value>,
        knot_id: &str,
        path: &Path,
    ) -> Result<(), SyncError> {
        let tag = required_string(data, "tag", path)?.trim().to_string();
        if !tag.is_empty() {
            self.apply_metadata_update(knot_id, |r| {
                if !r
                    .tags
                    .iter()
                    .any(|existing| existing.eq_ignore_ascii_case(&tag))
                {
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
        let tag = required_string(data, "tag", path)?.trim().to_string();
        if !tag.is_empty() {
            self.apply_metadata_update(knot_id, |r| {
                r.tags
                    .retain(|existing| !existing.eq_ignore_ascii_case(&tag));
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

#[cfg(test)]
#[path = "apply_tests_acceptance_ext.rs"]
mod tests_acceptance_ext;
#[cfg(test)]
#[path = "apply_tests_event_paths.rs"]
mod tests_event_paths;
#[cfg(test)]
#[path = "apply_tests_execution_plan.rs"]
mod tests_execution_plan;
#[cfg(test)]
#[path = "apply_tests_execution_plan_ext.rs"]
mod tests_execution_plan_ext;
#[cfg(test)]
#[path = "apply_tests_ext.rs"]
mod tests_ext;
#[cfg(test)]
#[path = "apply_tests_invariant.rs"]
mod tests_invariant;
#[cfg(test)]
#[path = "apply_tests_lease_expiry.rs"]
mod tests_lease_expiry;
#[cfg(test)]
#[path = "apply_tests_legacy_defaults.rs"]
mod tests_legacy_defaults;
#[cfg(test)]
#[path = "apply_tests_quarantine.rs"]
mod tests_quarantine;
#[cfg(test)]
#[path = "apply_tests_step_history.rs"]
mod tests_step_history;
#[cfg(test)]
#[path = "apply_tests_unknown_workflow.rs"]
mod tests_unknown_workflow;
