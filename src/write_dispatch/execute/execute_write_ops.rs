use std::path::Path;

use crate::app::{App, AppError, UpdateKnotPatch};
use crate::dispatch::{knot_ref, resolve_next_state};
use crate::domain::execution_plan::ExecutionPlanData;
use crate::domain::knot_type::KnotType;
use crate::domain::metadata::MetadataEntryInput;
use crate::lease_guard::{release_bound_lease, validate_next_bound_lease};
use crate::rollback::resolve_rollback_state;
use crate::ui;
use crate::write_queue::NextOperation;

use crate::write_dispatch::helpers::{
    execute_with_terminal_cascade_prompt, format_next_output, format_rollback_output,
    lease_state_actor, normalize_expected_state, parse_gate_failure_modes_option,
    parse_gate_owner_kind_arg, resolve_lease_agent_info, state_actor_from_agent_info,
    supplied_agent_flag_names, validate_non_claim_lease, warn_deprecated_agent_metadata,
};

pub(super) fn execute_update(
    app: &App,
    args: &crate::write_queue::UpdateOperation,
) -> Result<String, AppError> {
    let knot = app
        .show_knot(&args.id)?
        .ok_or_else(|| AppError::NotFound(args.id.clone()))?;
    let knot = if crate::lease_guard::materialize_expired_lease(app, &knot)? {
        app.show_knot(&args.id)?
            .ok_or_else(|| AppError::NotFound(args.id.clone()))?
    } else {
        knot
    };
    validate_non_claim_lease(&knot, args.lease_id.as_deref())?;
    let patch = build_update_patch(app, args)?;
    let scope_patch = super::scope_validation::parse_scope_patch(&args.scope)?;
    if !patch.has_changes() && !scope_patch.has_changes() {
        return Err(AppError::InvalidArgument(
            "update requires at least one field change".to_string(),
        ));
    }
    let had_field_changes = patch.has_changes();
    let knot = if had_field_changes {
        execute_with_terminal_cascade_prompt(
            args.approve_terminal_cascade,
            |approve_terminal_cascade| {
                app.update_knot_with_options(&args.id, patch.clone(), approve_terminal_cascade)
            },
        )?
    } else {
        knot
    };
    let knot = if scope_patch.has_changes() {
        let expected = (!had_field_changes)
            .then_some(args.if_match.as_deref())
            .flatten();
        app.update_knot_scope(&knot.id, scope_patch, expected)?
    } else {
        knot
    };
    refresh_lease_heartbeat(app, &knot);
    if args.json {
        return Ok(super::format_json(
            &serde_json::to_value(&knot).expect("updated knot should serialize"),
        ));
    }
    let palette = ui::Palette::auto();
    Ok(format!(
        "updated {} {} {}\n",
        palette.id(&knot_ref(&knot)),
        palette.state(&knot.state),
        knot.title
    ))
}

/// Refresh the lease expiry when a write command touches a bound knot.
fn refresh_lease_heartbeat(app: &App, knot: &crate::app::KnotView) {
    let Some(lease_id) = knot.lease_id.as_deref() else {
        return;
    };
    let Ok(Some(lease)) = app.show_knot(lease_id) else {
        return;
    };
    let state = crate::lease_expiry::effective_lease_state(&lease.state, lease.lease_expiry_ts);
    if state != crate::workflow_runtime::LEASE_ACTIVE {
        return;
    }
    let timeout = lease
        .lease
        .as_ref()
        .and_then(|d| d.timeout_seconds)
        .unwrap_or(crate::lease_expiry::DEFAULT_LEASE_TIMEOUT_SECONDS);
    let new_ts = crate::lease_expiry::compute_expiry_ts(timeout);
    let _ = app.set_lease_expiry(lease_id, new_ts);
}

fn build_update_patch(
    app: &App,
    args: &crate::write_queue::UpdateOperation,
) -> Result<UpdateKnotPatch, AppError> {
    use crate::domain::invariant::parse_invariant_spec;

    let lease_agent = resolve_lease_agent_info(app, &args.id);
    warn_deprecated_update_agent_metadata(args, lease_agent.is_some());
    let add_note = build_note_input(args, lease_agent.as_ref());
    let add_handoff_capsule = build_handoff_input(args, lease_agent.as_ref());
    let state_actor = state_actor_from_agent_info(args.actor_kind.clone(), lease_agent.as_ref());
    Ok(UpdateKnotPatch {
        title: args.title.clone(),
        description: args.description.clone(),
        acceptance: args.acceptance.clone(),
        priority: args.priority,
        status: args.status.clone(),
        knot_type: args
            .knot_type
            .as_deref()
            .map(|raw| raw.parse::<KnotType>().unwrap_or_default()),
        add_tags: args.add_tags.clone(),
        remove_tags: args.remove_tags.clone(),
        add_invariants: args
            .add_invariants
            .iter()
            .map(|raw| {
                parse_invariant_spec(raw).map_err(|err| AppError::InvalidArgument(err.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?,
        remove_invariants: args
            .remove_invariants
            .iter()
            .map(|raw| {
                parse_invariant_spec(raw).map_err(|err| AppError::InvalidArgument(err.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?,
        clear_invariants: args.clear_invariants,
        add_verification_steps: args.add_verification_steps.clone(),
        remove_verification_steps: args.remove_verification_steps.clone(),
        clear_verification_steps: args.clear_verification_steps,
        gate_owner_kind: parse_gate_owner_kind_arg(args.gate_owner_kind.as_deref())?,
        gate_failure_modes: parse_gate_failure_modes_option(&args.gate_failure_modes)?,
        clear_gate_failure_modes: args.clear_gate_failure_modes,
        execution_plan_objective: args.objective.clone(),
        execution_plan_data: load_execution_plan_data(args.execution_plan_file.as_deref())?,
        add_note,
        add_handoff_capsule,
        expected_profile_etag: args.if_match.clone(),
        force: args.force,
        state_actor,
    })
}

fn warn_deprecated_update_agent_metadata(
    args: &crate::write_queue::UpdateOperation,
    lease_bound: bool,
) {
    let mut flags = supplied_agent_flag_names(
        args.agent_name.as_deref(),
        args.agent_model.as_deref(),
        args.agent_version.as_deref(),
    );
    if args.note_agentname.is_some() {
        flags.push("note-agentname");
    }
    if args.note_model.is_some() {
        flags.push("note-model");
    }
    if args.note_version.is_some() {
        flags.push("note-version");
    }
    if args.handoff_agentname.is_some() {
        flags.push("handoff-agentname");
    }
    if args.handoff_model.is_some() {
        flags.push("handoff-model");
    }
    if args.handoff_version.is_some() {
        flags.push("handoff-version");
    }
    warn_deprecated_agent_metadata("update", &flags, lease_bound);
}

fn load_execution_plan_data(path: Option<&str>) -> Result<Option<ExecutionPlanData>, AppError> {
    let Some(path) = path else {
        return Ok(None);
    };
    let resolved = Path::new(path);
    let bytes = std::fs::read(resolved).map_err(|err| {
        AppError::InvalidArgument(format!(
            "failed to read execution plan file '{}': {}",
            resolved.display(),
            err
        ))
    })?;
    let payload = serde_json::from_slice(&bytes).map_err(|err| {
        AppError::InvalidArgument(format!(
            "invalid execution plan JSON in '{}': {}",
            resolved.display(),
            err
        ))
    })?;
    Ok(Some(payload))
}

fn build_note_input(
    args: &crate::write_queue::UpdateOperation,
    lai: Option<&crate::domain::lease::AgentInfo>,
) -> Option<MetadataEntryInput> {
    // Lease is the declared source of agent identity. Per-note agent flags
    // (--note-agentname, --note-model, --note-version) are deprecated and
    // ignored; the deprecation warning is emitted by
    // `warn_deprecated_update_agent_metadata`. `--note-username` remains a
    // caller-supplied override because it is not agent identity.
    args.add_note.clone().map(|content| MetadataEntryInput {
        content,
        username: args
            .note_username
            .clone()
            .or_else(|| lai.map(|i| i.provider.clone())),
        datetime: args.note_datetime.clone(),
        agentname: lai.map(|i| i.agent_name.clone()),
        model: lai.map(|i| i.model.clone()),
        version: lai.map(|i| i.model_version.clone()),
    })
}

fn build_handoff_input(
    args: &crate::write_queue::UpdateOperation,
    lai: Option<&crate::domain::lease::AgentInfo>,
) -> Option<MetadataEntryInput> {
    // Lease is the declared source of agent identity. Per-handoff agent flags
    // (--handoff-agentname, --handoff-model, --handoff-version) are deprecated
    // and ignored. `--handoff-username` remains a caller-supplied override
    // because it is not agent identity.
    args.add_handoff_capsule
        .clone()
        .map(|content| MetadataEntryInput {
            content,
            username: args
                .handoff_username
                .clone()
                .or_else(|| lai.map(|i| i.provider.clone())),
            datetime: args.handoff_datetime.clone(),
            agentname: lai.map(|i| i.agent_name.clone()),
            model: lai.map(|i| i.model.clone()),
            version: lai.map(|i| i.model_version.clone()),
        })
}

pub(super) fn execute_next(app: &App, args: &NextOperation) -> Result<String, AppError> {
    let knot = app
        .show_knot(&args.id)?
        .ok_or_else(|| AppError::NotFound(args.id.clone()))?;
    validate_next_preconditions(app, &knot, args)?;
    let lease_bound = knot.lease_id.is_some();
    warn_deprecated_agent_metadata(
        "next",
        &supplied_agent_flag_names(
            args.agent_name.as_deref(),
            args.agent_model.as_deref(),
            args.agent_version.as_deref(),
        ),
        lease_bound,
    );
    let (knot, next, owner_kind) = resolve_next_state(app, &knot.id)?;
    let previous_state = knot.state.clone();
    let actor = lease_state_actor(app, &knot.id, args.actor_kind.clone());
    let updated = execute_with_terminal_cascade_prompt(
        args.approve_terminal_cascade,
        |approve_terminal_cascade| {
            app.set_state_with_actor_and_options(
                &knot.id,
                &next,
                false,
                None,
                actor.clone(),
                approve_terminal_cascade,
                false,
            )
        },
    )?;
    if updated.lease_id.is_some() {
        release_bound_lease(app, &updated.id)?;
    }
    Ok(format_next_output(
        &updated,
        &previous_state,
        owner_kind,
        args.json,
    ))
}

fn validate_next_preconditions(
    app: &App,
    knot: &crate::app::KnotView,
    args: &NextOperation,
) -> Result<(), AppError> {
    if let Some(expected_raw) = args.expected_state.as_deref() {
        let expected = normalize_expected_state(expected_raw);
        if knot.state != expected {
            return Err(AppError::InvalidArgument(format!(
                "expected state '{expected}' but knot is \
                 currently '{}'",
                knot.state
            )));
        }
    }
    validate_next_bound_lease(app, knot, args.lease_id.as_deref())
}

pub(super) fn execute_rollback(
    app: &App,
    args: &crate::write_queue::RollbackOperation,
) -> Result<String, AppError> {
    if let Some(outcome) = args.outcome.as_deref() {
        return super::execute_failure_ops::execute_rollback_outcome(app, args, outcome);
    }
    let resolution = resolve_rollback_state(app, &args.id)?;
    // Generic rollback has always released whatever lease was bound without
    // being told which one; only validate when the caller opts in by
    // passing `--lease`.
    if args.lease_id.is_some() {
        validate_next_bound_lease(app, &resolution.knot, args.lease_id.as_deref())?;
    }
    let lease_bound = resolution.knot.lease_id.is_some();
    warn_deprecated_agent_metadata(
        "rollback",
        &supplied_agent_flag_names(
            args.agent_name.as_deref(),
            args.agent_model.as_deref(),
            args.agent_version.as_deref(),
        ),
        lease_bound,
    );
    if args.dry_run {
        if args.json {
            return Ok(super::format_json(&serde_json::json!({
                "id": resolution.knot.id,
                "state": resolution.knot.state,
                "target_state": resolution.target_state,
                "owner_kind": resolution.owner_kind,
                "reason": resolution.reason,
                "dry_run": true,
            })));
        }
        return Ok(format_rollback_output(
            &resolution.knot,
            &resolution.target_state,
            resolution.owner_kind,
            &resolution.reason,
            true,
        ));
    }
    let actor = lease_state_actor(app, &resolution.knot.id, args.actor_kind.clone());
    let updated = app.set_state_with_actor_and_options(
        &resolution.knot.id,
        &resolution.target_state,
        resolution.requires_force,
        None,
        actor,
        false,
        false,
    )?;
    if updated.lease_id.is_some() {
        release_bound_lease(app, &updated.id)?;
    }
    if args.json {
        return Ok(super::format_json(&serde_json::json!({
            "id": updated.id,
            "state": updated.state,
            "target_state": resolution.target_state,
            "owner_kind": resolution.owner_kind,
            "reason": resolution.reason,
            "dry_run": false,
        })));
    }
    Ok(format_rollback_output(
        &updated,
        &resolution.target_state,
        resolution.owner_kind,
        &resolution.reason,
        false,
    ))
}
