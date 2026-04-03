use std::time::Duration;

use crate::db::{self, KnotCacheRecord, UpsertKnotHot};
use crate::domain::knot_type::parse_knot_type;
use crate::events::{
    new_event_id, now_utc_rfc3339, EventRecord, FullEvent, FullEventKind, IndexEvent,
    IndexEventKind,
};
use crate::locks::FileLock;
use crate::state_hierarchy::{self, HierarchyKnot, TransitionPlan};
use crate::workflow_runtime;

use super::error::AppError;
use super::helpers::{
    apply_step_transition, build_knot_head_data, build_state_event_data, ensure_profile_etag,
    next_blocked_from_state, next_deferred_from_state, normalize_state_input,
    resolve_step_metadata, KnotHeadData, StateCascadeMetadata, StateEventParams,
};
use super::immutable_records::ensure_append_only_step_history;
use super::types::{KnotView, StateActorMetadata};
use super::App;

impl App {
    #[allow(dead_code)]
    pub fn set_state(
        &self,
        id: &str,
        next_state: &str,
        force: bool,
        expected_profile_etag: Option<&str>,
    ) -> Result<KnotView, AppError> {
        self.set_state_with_actor(
            id,
            next_state,
            force,
            expected_profile_etag,
            StateActorMetadata::default(),
        )
    }

    pub fn set_state_with_actor(
        &self,
        id: &str,
        next_state: &str,
        force: bool,
        expected_profile_etag: Option<&str>,
        state_actor: StateActorMetadata,
    ) -> Result<KnotView, AppError> {
        self.set_state_with_actor_and_options(
            id,
            next_state,
            force,
            expected_profile_etag,
            state_actor,
            false,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn set_state_with_actor_and_options(
        &self,
        id: &str,
        next_state: &str,
        force: bool,
        expected_profile_etag: Option<&str>,
        state_actor: StateActorMetadata,
        approve_terminal_cascade: bool,
        skip_hierarchy_progress_check: bool,
    ) -> Result<KnotView, AppError> {
        let id = self.resolve_knot_token(id)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        let current =
            db::get_knot_hot(&self.conn, &id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
        ensure_profile_etag(&current, expected_profile_etag)?;
        let next = normalize_state_input(next_state)?;
        let updated = self.apply_state_transition_locked(
            &current,
            &next,
            force,
            expected_profile_etag,
            &state_actor,
            approve_terminal_cascade,
            skip_hierarchy_progress_check,
        )?;
        if self.transitioned_to_terminal_resolution_state(&current, &updated)? {
            self.auto_resolve_terminal_parents_locked([updated.id.as_str()])?;
        }
        self.apply_alias_and_enrich_knot(KnotView::from(updated))
    }

    pub(crate) fn reconcile_terminal_parent_state(
        &self,
        id: &str,
        next_state: &str,
    ) -> Result<KnotView, AppError> {
        let id = self.resolve_knot_token(id)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        let current =
            db::get_knot_hot(&self.conn, &id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
        let next_state = normalize_state_input(next_state)?;
        let updated = self.reconcile_terminal_parent_state_locked(&current, &next_state)?;
        if self.transitioned_to_terminal_resolution_state(&current, &updated)? {
            self.auto_resolve_terminal_parents_locked([updated.id.as_str()])?;
        }
        self.apply_alias_and_enrich_knot(KnotView::from(updated))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_state_transition_locked(
        &self,
        current: &KnotCacheRecord,
        next_state: &str,
        force: bool,
        expected_profile_etag: Option<&str>,
        state_actor: &StateActorMetadata,
        approve_terminal_cascade: bool,
        skip_hierarchy_progress_check: bool,
    ) -> Result<KnotCacheRecord, AppError> {
        let profile = self.resolve_profile_for_record(current)?;
        let knot_type = parse_knot_type(current.knot_type.as_deref());
        let next_is_terminal = workflow_runtime::is_terminal_state(
            &self.profile_registry,
            &profile.id,
            knot_type,
            next_state,
        )?;
        self.validate_resume_or_transition(current, next_state, force, next_is_terminal)?;
        match state_hierarchy::plan_state_transition(
            &self.conn,
            current,
            next_state,
            next_is_terminal,
            approve_terminal_cascade,
            skip_hierarchy_progress_check,
        )? {
            TransitionPlan::Allowed => self.write_state_change_locked(
                current,
                next_state,
                force,
                expected_profile_etag,
                state_actor,
                None,
            ),
            TransitionPlan::CascadeTerminal { descendants } => self.cascade_terminal_state_locked(
                current,
                next_state,
                expected_profile_etag,
                state_actor,
                &descendants,
                force,
            ),
        }
    }

    fn validate_resume_or_transition(
        &self,
        current: &KnotCacheRecord,
        next_state: &str,
        force: bool,
        next_is_terminal: bool,
    ) -> Result<(), AppError> {
        let resuming_deferred = current.state == "deferred" && next_state != "deferred";
        let resuming_blocked = current.state == "blocked" && next_state != "blocked";
        if !force && !next_is_terminal && (resuming_deferred || resuming_blocked) {
            validate_resume_provenance(current, next_state, resuming_deferred, resuming_blocked)
        } else {
            let profile = self.resolve_profile_for_record(current)?;
            let knot_type = parse_knot_type(current.knot_type.as_deref());
            workflow_runtime::validate_transition(
                &self.profile_registry,
                &profile.id,
                knot_type,
                &current.state,
                next_state,
                force,
            )?;
            Ok(())
        }
    }

    pub(crate) fn cascade_terminal_state_locked(
        &self,
        current: &KnotCacheRecord,
        next_state: &str,
        expected_profile_etag: Option<&str>,
        state_actor: &StateActorMetadata,
        descendants: &[HierarchyKnot],
        force_root: bool,
    ) -> Result<KnotCacheRecord, AppError> {
        let cascade = StateCascadeMetadata {
            root_id: &current.id,
        };
        let mut changed_ids = Vec::new();
        for descendant in descendants {
            let Some(rec) = db::get_knot_hot(&self.conn, &descendant.id)? else {
                continue;
            };
            if state_hierarchy::is_terminal_state(&rec.state)? {
                continue;
            }
            self.write_state_change_locked(
                &rec,
                next_state,
                false,
                None,
                state_actor,
                Some(cascade),
            )?;
            changed_ids.push(rec.id);
        }
        let updated = self.write_state_change_locked(
            current,
            next_state,
            force_root,
            expected_profile_etag,
            state_actor,
            Some(cascade),
        )?;
        changed_ids.push(updated.id.clone());
        self.auto_resolve_terminal_parents_locked(changed_ids.iter().map(String::as_str))?;
        Ok(updated)
    }

    pub(crate) fn write_state_change_locked(
        &self,
        current: &KnotCacheRecord,
        next_state: &str,
        force: bool,
        expected_profile_etag: Option<&str>,
        state_actor: &StateActorMetadata,
        cascade: Option<StateCascadeMetadata<'_>>,
    ) -> Result<KnotCacheRecord, AppError> {
        let profile = self.resolve_profile_for_record(current)?;
        let profile_id = profile.id.clone();
        let knot_type = parse_knot_type(current.knot_type.as_deref());
        let deferred = next_deferred_from_state(current, next_state);
        let blocked = next_blocked_from_state(profile, current, next_state);
        let occurred_at = now_utc_rfc3339();
        let (full_event, idx_id, idx_event) = self.build_state_change_events(
            current,
            next_state,
            &profile.workflow_id,
            &profile_id,
            force,
            deferred.as_deref(),
            blocked.as_deref(),
            state_actor,
            cascade,
            expected_profile_etag,
            &occurred_at,
            knot_type,
        )?;
        self.writer.write(&EventRecord::full(full_event))?;
        self.writer.write(&EventRecord::index(idx_event))?;
        let step_history = apply_step_transition(
            &current.step_history,
            &current.state,
            next_state,
            &occurred_at,
            state_actor,
            current.lease_id.as_deref(),
        );
        ensure_append_only_step_history(&current.step_history, &step_history)?;
        db::upsert_knot_hot(
            &self.conn,
            &UpsertKnotHot {
                id: &current.id,
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
                step_history: &step_history,
                gate_data: &current.gate_data,
                lease_data: &current.lease_data,
                lease_id: current.lease_id.as_deref(),
                workflow_id: &profile.workflow_id,
                profile_id: &profile_id,
                profile_etag: Some(&idx_id),
                deferred_from_state: deferred.as_deref(),
                blocked_from_state: blocked.as_deref(),
                created_at: current.created_at.as_deref(),
            },
        )?;
        let updated = db::get_knot_hot(&self.conn, &current.id)?
            .ok_or_else(|| AppError::NotFound(current.id.clone()))?;
        if next_state == "shipped" {
            self.resume_blocked_dependents_locked(&updated.id, state_actor)?;
        }
        Ok(updated)
    }

    #[allow(clippy::too_many_arguments)]
    fn build_state_change_events(
        &self,
        current: &KnotCacheRecord,
        next_state: &str,
        workflow_id: &str,
        profile_id: &str,
        force: bool,
        deferred: Option<&str>,
        blocked: Option<&str>,
        state_actor: &StateActorMetadata,
        cascade: Option<StateCascadeMetadata<'_>>,
        expected_profile_etag: Option<&str>,
        occurred_at: &str,
        knot_type: crate::domain::knot_type::KnotType,
    ) -> Result<(FullEvent, String, IndexEvent), AppError> {
        let data = build_state_event_data(&StateEventParams {
            from: &current.state,
            to: next_state,
            workflow_id,
            profile_id,
            force,
            deferred_from_state: deferred,
            blocked_from_state: blocked,
            state_actor,
            cascade,
        })?;
        let mut full_event = FullEvent::with_identity(
            new_event_id(),
            occurred_at.to_string(),
            current.id.clone(),
            FullEventKind::KnotStateSet.as_str(),
            data,
        );
        if let Some(expected) = expected_profile_etag {
            full_event = full_event.with_precondition(expected);
        }
        let idx_id = new_event_id();
        let terminal = workflow_runtime::is_terminal_state(
            &self.profile_registry,
            profile_id,
            knot_type,
            next_state,
        )?;
        let (step_metadata, next_step_metadata) = resolve_step_metadata(
            &self.profile_registry,
            workflow_id,
            profile_id,
            knot_type,
            &current.gate_data,
            next_state,
        )?;
        let mut idx_event = IndexEvent::with_identity(
            idx_id.clone(),
            occurred_at.to_string(),
            IndexEventKind::KnotHead.as_str(),
            build_knot_head_data(KnotHeadData {
                knot_id: &current.id,
                title: &current.title,
                state: next_state,
                workflow_id,
                profile_id,
                updated_at: occurred_at,
                terminal,
                deferred_from_state: deferred,
                blocked_from_state: blocked,
                invariants: &current.invariants,
                knot_type,
                gate_data: &current.gate_data,
                step_metadata: step_metadata.as_ref(),
                next_step_metadata: next_step_metadata.as_ref(),
            }),
        );
        if let Some(expected) = expected_profile_etag {
            idx_event = idx_event.with_precondition(expected);
        }
        Ok((full_event, idx_id, idx_event))
    }
}

fn validate_resume_provenance(
    current: &KnotCacheRecord,
    next_state: &str,
    resuming_deferred: bool,
    resuming_blocked: bool,
) -> Result<(), AppError> {
    if resuming_deferred {
        let expected = current.deferred_from_state.as_deref().ok_or_else(|| {
            AppError::InvalidArgument(
                "deferred knot is missing \
                     deferred_from_state provenance"
                    .to_string(),
            )
        })?;
        if expected != next_state {
            return Err(AppError::InvalidArgument(format!(
                "deferred knots may only resume to '{}'",
                expected
            )));
        }
    }
    if resuming_blocked {
        let expected = current.blocked_from_state.as_deref().ok_or_else(|| {
            AppError::InvalidArgument(
                "blocked knot is missing \
                     blocked_from_state provenance"
                    .to_string(),
            )
        })?;
        if expected != next_state {
            return Err(AppError::InvalidArgument(format!(
                "blocked knots may only resume to '{}'",
                expected
            )));
        }
    }
    Ok(())
}
