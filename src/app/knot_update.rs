use std::time::Duration;

use crate::db::{self, KnotCacheRecord, UpsertKnotHot};
use crate::domain::knot_type::parse_knot_type;
use crate::events::{
    new_event_id, now_utc_rfc3339, EventRecord, FullEvent, FullEventKind, IndexEvent,
    IndexEventKind,
};
use crate::locks::FileLock;
use crate::state_hierarchy;
use crate::workflow_runtime;

use super::error::AppError;
use super::helpers::{
    apply_step_transition, build_knot_head_data, build_state_event_data, ensure_profile_etag,
    next_blocked_from_state, next_deferred_from_state, normalize_state_input,
    resolve_step_metadata, KnotHeadData, StateEventParams,
};
use super::immutable_records::{ensure_append_only_metadata, ensure_append_only_step_history};
use super::types::{KnotView, UpdateKnotPatch};
use super::App;

mod fields;

impl App {
    pub fn update_knot(&self, id: &str, patch: UpdateKnotPatch) -> Result<KnotView, AppError> {
        self.update_knot_with_options(id, patch, false)
    }

    pub(crate) fn update_knot_with_options(
        &self,
        id: &str,
        patch: UpdateKnotPatch,
        approve_terminal_cascade: bool,
    ) -> Result<KnotView, AppError> {
        let id = self.resolve_knot_token(id)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        if !patch.has_changes() {
            return Err(AppError::InvalidArgument(
                "update requires at least one field change".to_string(),
            ));
        }
        let current =
            db::get_knot_hot(&self.conn, &id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
        ensure_profile_etag(&current, patch.expected_profile_etag.as_deref())?;
        update_knot_locked(self, &id, current, patch, approve_terminal_cascade)
    }
}

struct UpdateState {
    title: String,
    state: String,
    description: Option<String>,
    body: Option<String>,
    acceptance: Option<String>,
    priority: Option<i64>,
    knot_type: crate::domain::knot_type::KnotType,
    deferred: Option<String>,
    blocked: Option<String>,
    tags: Vec<String>,
    notes: Vec<crate::domain::metadata::MetadataEntry>,
    handoff_capsules: Vec<crate::domain::metadata::MetadataEntry>,
    invariants: Vec<crate::domain::invariant::Invariant>,
    gate_data: crate::domain::gate::GateData,
    current_precondition: Option<String>,
}

impl UpdateState {
    fn from_record(record: &KnotCacheRecord, precondition: Option<String>) -> Self {
        Self {
            title: record.title.clone(),
            state: record.state.clone(),
            description: record.description.clone(),
            body: record.body.clone(),
            acceptance: record.acceptance.clone(),
            priority: record.priority,
            knot_type: parse_knot_type(record.knot_type.as_deref()),
            deferred: record.deferred_from_state.clone(),
            blocked: record.blocked_from_state.clone(),
            tags: record.tags.clone(),
            notes: record.notes.clone(),
            handoff_capsules: record.handoff_capsules.clone(),
            invariants: record.invariants.clone(),
            gate_data: record.gate_data.clone(),
            current_precondition: precondition,
        }
    }

    fn refresh_from_record(&mut self, record: &KnotCacheRecord) {
        self.title = record.title.clone();
        self.state = record.state.clone();
        self.description = record.description.clone();
        self.body = record.body.clone();
        self.acceptance = record.acceptance.clone();
        self.priority = record.priority;
        self.knot_type = parse_knot_type(record.knot_type.as_deref());
        self.deferred = record.deferred_from_state.clone();
        self.blocked = record.blocked_from_state.clone();
        self.tags = record.tags.clone();
        self.notes = record.notes.clone();
        self.handoff_capsules = record.handoff_capsules.clone();
        self.invariants = record.invariants.clone();
        self.gate_data = record.gate_data.clone();
    }
}

fn update_knot_locked(
    app: &App,
    id: &str,
    mut current: KnotCacheRecord,
    patch: UpdateKnotPatch,
    approve_terminal_cascade: bool,
) -> Result<KnotView, AppError> {
    let profile = app.resolve_profile_for_record(&current)?;
    let profile_id = profile.id.clone();
    let occurred_at = now_utc_rfc3339();
    let mut us = UpdateState::from_record(&current, patch.expected_profile_etag.clone());
    let mut full_events = Vec::new();

    if let Some(next_raw) = patch.status.as_deref() {
        apply_status_change(
            app,
            &mut current,
            &mut us,
            &mut full_events,
            next_raw,
            &profile_id,
            profile,
            &patch,
            approve_terminal_cascade,
            id,
            &occurred_at,
        )?;
    }

    fields::collect_field_events(
        &patch,
        &mut full_events,
        id,
        &occurred_at,
        &mut us,
        &current,
    )?;

    if full_events.is_empty() {
        return app.apply_alias_and_enrich_knot(KnotView::from(current));
    }

    write_update_events_and_cache(
        app,
        id,
        &current,
        &us,
        full_events,
        &profile.workflow_id,
        &profile_id,
        &occurred_at,
        &patch,
    )?;

    let updated =
        db::get_knot_hot(&app.conn, id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
    if app.transitioned_to_terminal_resolution_state(&current, &updated)? {
        app.auto_resolve_terminal_parents_locked([updated.id.as_str()])?;
    }
    app.apply_alias_and_enrich_knot(KnotView::from(updated))
}

#[allow(clippy::too_many_arguments)]
fn apply_status_change(
    app: &App,
    current: &mut KnotCacheRecord,
    us: &mut UpdateState,
    full_events: &mut Vec<FullEvent>,
    next_raw: &str,
    profile_id: &str,
    profile: &crate::workflow::ProfileDefinition,
    patch: &UpdateKnotPatch,
    approve_terminal_cascade: bool,
    id: &str,
    occurred_at: &str,
) -> Result<(), AppError> {
    let next_state = normalize_state_input(next_raw)?;
    let next_is_terminal = workflow_runtime::is_terminal_state(
        &app.profile_registry,
        profile_id,
        us.knot_type,
        &next_state,
    )?;
    validate_resume(
        current,
        &next_state,
        patch.force,
        next_is_terminal,
        profile_id,
        us.knot_type,
        &us.deferred,
        &us.blocked,
        app,
    )?;
    match state_hierarchy::plan_state_transition(
        &app.conn,
        current,
        &next_state,
        next_is_terminal,
        approve_terminal_cascade,
        false,
    )? {
        state_hierarchy::TransitionPlan::Allowed if us.state != next_state => {
            us.deferred = next_deferred_from_state(current, &next_state);
            us.blocked = next_blocked_from_state(profile, current, &next_state);
            us.state = next_state.clone();
            let data = build_state_event_data(&StateEventParams {
                from: &current.state,
                to: &us.state,
                workflow_id: &profile.workflow_id,
                profile_id,
                force: patch.force,
                deferred_from_state: us.deferred.as_deref(),
                blocked_from_state: us.blocked.as_deref(),
                state_actor: &patch.state_actor,
                cascade: None,
            })?;
            full_events.push(FullEvent::with_identity(
                new_event_id(),
                occurred_at.to_string(),
                id.to_string(),
                FullEventKind::KnotStateSet.as_str(),
                data,
            ));
        }
        state_hierarchy::TransitionPlan::Allowed => {}
        state_hierarchy::TransitionPlan::CascadeTerminal { descendants } => {
            *current = app.cascade_terminal_state_locked(
                current,
                &next_state,
                patch.expected_profile_etag.as_deref(),
                &patch.state_actor,
                &descendants,
                patch.force,
            )?;
            us.current_precondition = current.profile_etag.clone();
            us.refresh_from_record(current);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_resume(
    current: &KnotCacheRecord,
    next_state: &str,
    force: bool,
    next_is_terminal: bool,
    profile_id: &str,
    knot_type: crate::domain::knot_type::KnotType,
    deferred: &Option<String>,
    blocked: &Option<String>,
    app: &App,
) -> Result<(), AppError> {
    let resuming_deferred = current.state == "deferred" && next_state != "deferred";
    let resuming_blocked = current.state == "blocked" && next_state != "blocked";
    if !force && !next_is_terminal && (resuming_deferred || resuming_blocked) {
        if resuming_deferred {
            let expected = deferred.as_deref().ok_or_else(|| {
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
            let expected = blocked.as_deref().ok_or_else(|| {
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
    } else {
        workflow_runtime::validate_transition(
            &app.profile_registry,
            profile_id,
            knot_type,
            &current.state,
            next_state,
            force,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_update_events_and_cache(
    app: &App,
    id: &str,
    current: &KnotCacheRecord,
    us: &UpdateState,
    full_events: Vec<FullEvent>,
    workflow_id: &str,
    profile_id: &str,
    occurred_at: &str,
    patch: &UpdateKnotPatch,
) -> Result<(), AppError> {
    for mut event in full_events {
        if let Some(expected) = us.current_precondition.as_deref() {
            event = event.with_precondition(expected);
        }
        app.writer.write(&EventRecord::full(event))?;
    }
    let terminal = workflow_runtime::is_terminal_state(
        &app.profile_registry,
        profile_id,
        us.knot_type,
        &us.state,
    )?;
    let (step_metadata, next_step_metadata) = resolve_step_metadata(
        &app.profile_registry,
        workflow_id,
        profile_id,
        us.knot_type,
        &us.gate_data,
        &us.state,
    )?;
    let index_event_id = new_event_id();
    let mut idx_event = IndexEvent::with_identity(
        index_event_id.clone(),
        occurred_at.to_string(),
        IndexEventKind::KnotHead.as_str(),
        build_knot_head_data(KnotHeadData {
            knot_id: id,
            title: &us.title,
            state: &us.state,
            workflow_id,
            profile_id,
            updated_at: occurred_at,
            terminal,
            deferred_from_state: us.deferred.as_deref(),
            blocked_from_state: us.blocked.as_deref(),
            invariants: &us.invariants,
            knot_type: us.knot_type,
            gate_data: &us.gate_data,
            step_metadata: step_metadata.as_ref(),
            next_step_metadata: next_step_metadata.as_ref(),
        }),
    );
    if let Some(expected) = us.current_precondition.as_deref() {
        idx_event = idx_event.with_precondition(expected);
    }
    app.writer.write(&EventRecord::index(idx_event))?;
    let step_history = apply_step_transition(
        &current.step_history,
        &current.state,
        &us.state,
        occurred_at,
        &patch.state_actor,
        current.lease_id.as_deref(),
    );
    ensure_append_only_metadata(&current.notes, &us.notes, "note")?;
    ensure_append_only_metadata(
        &current.handoff_capsules,
        &us.handoff_capsules,
        "handoff capsule",
    )?;
    ensure_append_only_step_history(&current.step_history, &step_history)?;
    db::upsert_knot_hot(
        &app.conn,
        &UpsertKnotHot {
            id,
            title: &us.title,
            state: &us.state,
            updated_at: occurred_at,
            body: us.body.as_deref(),
            description: us.description.as_deref(),
            acceptance: us.acceptance.as_deref(),
            priority: us.priority,
            knot_type: Some(us.knot_type.as_str()),
            tags: &us.tags,
            notes: &us.notes,
            handoff_capsules: &us.handoff_capsules,
            invariants: &us.invariants,
            step_history: &step_history,
            gate_data: &us.gate_data,
            lease_data: &current.lease_data,
            lease_id: current.lease_id.as_deref(),
            workflow_id,
            profile_id,
            profile_etag: Some(&index_event_id),
            deferred_from_state: us.deferred.as_deref(),
            blocked_from_state: us.blocked.as_deref(),
            created_at: current.created_at.as_deref(),
        },
    )?;
    Ok(())
}
