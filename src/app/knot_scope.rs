use std::time::Duration;

use crate::db::{self, UpsertKnotHot};
use crate::domain::knot_type::parse_knot_type;
use crate::domain::scope::ScopePatch;
use crate::events::{
    new_event_id, now_utc_rfc3339, EventRecord, FullEvent, FullEventKind, IndexEvent,
    IndexEventKind,
};
use crate::locks::FileLock;
use crate::workflow_runtime;

use super::error::AppError;
use super::helpers::{
    build_knot_head_data, ensure_profile_etag, resolve_step_metadata, KnotHeadData,
};
use super::types::KnotView;
use super::App;

impl App {
    pub fn update_knot_scope(
        &self,
        id: &str,
        patch: ScopePatch,
        expected_profile_etag: Option<&str>,
    ) -> Result<KnotView, AppError> {
        if !patch.has_changes() {
            return Err(AppError::InvalidArgument(
                "scope update requires at least one field change".to_string(),
            ));
        }
        let id = self.resolve_knot_token(id)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        let current =
            db::get_knot_hot(&self.conn, &id)?.ok_or_else(|| AppError::NotFound(id.clone()))?;
        ensure_profile_etag(&current, expected_profile_etag)?;
        let next_scope = patch.apply_to(current.scope_data.clone());
        if next_scope == current.scope_data {
            return self.apply_alias_and_enrich_knot(KnotView::from(current));
        }
        self.write_scope_update(&id, &current, &next_scope)
    }

    fn write_scope_update(
        &self,
        id: &str,
        current: &db::KnotCacheRecord,
        scope_data: &crate::domain::scope::ScopeData,
    ) -> Result<KnotView, AppError> {
        let occurred_at = now_utc_rfc3339();
        let profile = self.resolve_profile_for_record(current)?;
        let knot_type = parse_knot_type(current.knot_type.as_deref());
        let terminal = workflow_runtime::is_terminal_state(
            &self.profile_registry,
            profile.id.as_str(),
            knot_type,
            &current.state,
        )?;
        let (step_metadata, next_step_metadata) = resolve_step_metadata(
            &self.profile_registry,
            &profile.workflow_id,
            profile.id.as_str(),
            knot_type,
            &current.gate_data,
            &current.state,
        )?;
        self.writer
            .write(&EventRecord::full(FullEvent::with_identity(
                new_event_id(),
                occurred_at.clone(),
                id.to_string(),
                FullEventKind::KnotScopeSet.as_str(),
                serde_json::to_value(scope_data).expect("scope data should serialize"),
            )))?;
        let index_event_id = new_event_id();
        let idx_event = IndexEvent::with_identity(
            index_event_id.clone(),
            occurred_at.clone(),
            IndexEventKind::KnotHead.as_str(),
            build_knot_head_data(KnotHeadData {
                knot_id: id,
                title: &current.title,
                state: &current.state,
                workflow_id: &profile.workflow_id,
                profile_id: profile.id.as_str(),
                updated_at: &occurred_at,
                terminal,
                deferred_from_state: current.deferred_from_state.as_deref(),
                blocked_from_state: current.blocked_from_state.as_deref(),
                invariants: &current.invariants,
                knot_type,
                gate_data: &current.gate_data,
                execution_plan_data: &current.execution_plan_data,
                scope_data: Some(scope_data),
                step_metadata: step_metadata.as_ref(),
                next_step_metadata: next_step_metadata.as_ref(),
                lease_expiry_ts: current.lease_expiry_ts,
            }),
        );
        self.writer.write(&EventRecord::index(idx_event))?;
        self.upsert_scope_cache(id, current, &occurred_at, &index_event_id, scope_data)
    }

    fn upsert_scope_cache(
        &self,
        id: &str,
        current: &db::KnotCacheRecord,
        occurred_at: &str,
        profile_etag: &str,
        scope_data: &crate::domain::scope::ScopeData,
    ) -> Result<KnotView, AppError> {
        db::upsert_knot_hot(
            &self.conn,
            &UpsertKnotHot {
                id,
                title: &current.title,
                state: &current.state,
                updated_at: occurred_at,
                body: current.body.as_deref(),
                description: current.description.as_deref(),
                acceptance: current.acceptance.as_deref(),
                priority: current.priority,
                knot_type: current.knot_type.as_deref(),
                tags: &current.tags,
                notes: &current.notes,
                handoff_capsules: &current.handoff_capsules,
                invariants: &current.invariants,
                verification_steps: &current.verification_steps,
                step_history: &current.step_history,
                gate_data: &current.gate_data,
                lease_data: &current.lease_data,
                execution_plan_data: &current.execution_plan_data,
                lease_id: current.lease_id.as_deref(),
                lease_expiry_ts: current.lease_expiry_ts,
                workflow_id: &current.workflow_id,
                profile_id: &current.profile_id,
                profile_etag: Some(profile_etag),
                deferred_from_state: current.deferred_from_state.as_deref(),
                blocked_from_state: current.blocked_from_state.as_deref(),
                created_at: current.created_at.as_deref(),
            },
        )?;
        db::update_knot_scope_data(&self.conn, id, scope_data)?;
        let updated =
            db::get_knot_hot(&self.conn, id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
        self.apply_alias_and_enrich_knot(KnotView::from(updated))
    }
}
