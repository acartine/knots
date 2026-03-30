use std::time::Duration;

use serde_json::json;

use crate::db::{self, KnotCacheRecord, UpsertKnotHot};
use crate::domain::knot_type::parse_knot_type;
use crate::events::{
    new_event_id, now_utc_rfc3339, EventRecord, FullEvent, FullEventKind, IndexEvent,
    IndexEventKind,
};
use crate::locks::FileLock;
use crate::workflow_runtime;

use super::error::AppError;
use super::helpers::{
    build_knot_head_data, ensure_profile_etag, next_blocked_from_state, next_deferred_from_state,
    normalize_state_input, require_state_for_knot_type, KnotHeadData,
};
use super::types::KnotView;
use super::App;

impl App {
    pub fn set_profile(
        &self,
        id: &str,
        profile_id: &str,
        state: &str,
        expected_profile_etag: Option<&str>,
    ) -> Result<KnotView, AppError> {
        let id = self.resolve_knot_token(id)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        let current =
            db::get_knot_hot(&self.conn, &id)?.ok_or_else(|| AppError::NotFound(id.clone()))?;
        ensure_profile_etag(&current, expected_profile_etag)?;
        let (profile, next_state) = self.resolve_set_profile_params(&current, profile_id, state)?;
        let current_pid =
            super::helpers::canonical_profile_id(&current.profile_id, &current.workflow_id);
        if current_pid == profile.id && current.state == next_state {
            return self.apply_alias_to_knot(KnotView::from(current));
        }
        self.write_set_profile_events(&id, &current, profile, &next_state, expected_profile_etag)
    }

    fn resolve_set_profile_params<'a>(
        &'a self,
        current: &KnotCacheRecord,
        profile_id: &str,
        state: &str,
    ) -> Result<(&'a crate::workflow::ProfileDefinition, String), AppError> {
        let current_profile = self.profile_registry.require(&current.profile_id)?;
        let current_wf = if current.workflow_id.trim().is_empty() {
            current_profile.workflow_id.clone()
        } else {
            current.workflow_id.clone()
        };
        let resolved = self.resolve_profile_id(profile_id, Some(&current_wf))?;
        let profile = self.profile_registry.require(&resolved)?;
        if profile.workflow_id != current_wf {
            return Err(AppError::InvalidArgument(format!(
                "cannot change knot '{}' from workflow '{}' \
                 to '{}'",
                current.id, current_wf, profile.workflow_id
            )));
        }
        let next_state = normalize_state_input(state)?;
        require_state_for_knot_type(
            parse_knot_type(current.knot_type.as_deref()),
            profile,
            &next_state,
        )?;
        Ok((profile, next_state))
    }

    fn write_set_profile_events(
        &self,
        id: &str,
        current: &KnotCacheRecord,
        profile: &crate::workflow::ProfileDefinition,
        next_state: &str,
        expected_profile_etag: Option<&str>,
    ) -> Result<KnotView, AppError> {
        let current_pid =
            super::helpers::canonical_profile_id(&current.profile_id, &current.workflow_id);
        let deferred = next_deferred_from_state(current, next_state);
        let blocked = next_blocked_from_state(profile, current, next_state);
        let occurred_at = now_utc_rfc3339();
        let mut full_event = FullEvent::with_identity(
            new_event_id(),
            occurred_at.clone(),
            id.to_string(),
            FullEventKind::KnotProfileSet.as_str(),
            json!({
                "from_profile_id": current_pid,
                "to_profile_id": profile.id,
                "from_state": current.state,
                "to_state": next_state,
                "deferred_from_state": deferred,
                "blocked_from_state": blocked,
            }),
        );
        if let Some(expected) = expected_profile_etag {
            full_event = full_event.with_precondition(expected);
        }
        let knot_type = parse_knot_type(current.knot_type.as_deref());
        let index_event_id = new_event_id();
        let terminal = workflow_runtime::is_terminal_state(
            &self.profile_registry,
            profile.id.as_str(),
            knot_type,
            next_state,
        )?;
        let mut idx_event = IndexEvent::with_identity(
            index_event_id.clone(),
            occurred_at.clone(),
            IndexEventKind::KnotHead.as_str(),
            build_knot_head_data(KnotHeadData {
                knot_id: id,
                title: &current.title,
                state: next_state,
                workflow_id: profile.workflow_id.as_str(),
                profile_id: profile.id.as_str(),
                updated_at: &occurred_at,
                terminal,
                deferred_from_state: deferred.as_deref(),
                blocked_from_state: blocked.as_deref(),
                invariants: &current.invariants,
                knot_type,
                gate_data: &current.gate_data,
            }),
        );
        if let Some(expected) = expected_profile_etag {
            idx_event = idx_event.with_precondition(expected);
        }
        self.writer.write(&EventRecord::full(full_event))?;
        self.writer.write(&EventRecord::index(idx_event))?;
        db::upsert_knot_hot(
            &self.conn,
            &UpsertKnotHot {
                id,
                title: &current.title,
                state: next_state,
                updated_at: &occurred_at,
                body: current.body.as_deref(),
                description: current.description.as_deref(),
                acceptance: current.acceptance.as_deref(),
                priority: current.priority,
                knot_type: current.knot_type.as_deref(),
                tags: &current.tags,
                notes: &current.notes,
                handoff_capsules: &current.handoff_capsules,
                invariants: &current.invariants,
                step_history: &current.step_history,
                gate_data: &current.gate_data,
                lease_data: &current.lease_data,
                lease_id: current.lease_id.as_deref(),
                workflow_id: &profile.workflow_id,
                profile_id: &profile.id,
                profile_etag: Some(&index_event_id),
                deferred_from_state: deferred.as_deref(),
                blocked_from_state: blocked.as_deref(),
                created_at: current.created_at.as_deref(),
            },
        )?;
        let updated =
            db::get_knot_hot(&self.conn, id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
        self.apply_alias_to_knot(KnotView::from(updated))
    }
}
