use crate::app::{App, AppError, StateActorMetadata, UpdateKnotPatch};
use crate::dispatch::{knot_ref, resolve_next_state};
use crate::domain::knot_type::KnotType;
use crate::domain::metadata::MetadataEntryInput;
use crate::rollback::resolve_rollback_state;
use crate::ui;
use crate::write_queue::NextOperation;

use crate::write_dispatch::helpers::{
    execute_with_terminal_cascade_prompt, format_next_output, format_rollback_output,
    normalize_expected_state, parse_gate_failure_modes_option, parse_gate_owner_kind_arg,
    resolve_lease_agent_info, validate_non_claim_lease,
};

pub(super) fn execute_update(
    app: &App,
    args: &crate::write_queue::UpdateOperation,
) -> Result<String, AppError> {
    let knot = app
        .show_knot(&args.id)?
        .ok_or_else(|| AppError::NotFound(args.id.clone()))?;
    validate_non_claim_lease(&knot, args.lease_id.as_deref())?;
    let patch = build_update_patch(app, args)?;
    let knot = execute_with_terminal_cascade_prompt(
        args.approve_terminal_cascade,
        |approve_terminal_cascade| {
            app.update_knot_with_options(&args.id, patch.clone(), approve_terminal_cascade)
        },
    )?;
    let palette = ui::Palette::auto();
    Ok(format!(
        "updated {} {} {}\n",
        palette.id(&knot_ref(&knot)),
        palette.state(&knot.state),
        knot.title
    ))
}

fn build_update_patch(
    app: &App,
    args: &crate::write_queue::UpdateOperation,
) -> Result<UpdateKnotPatch, AppError> {
    use crate::domain::invariant::parse_invariant_spec;

    let lease_agent = resolve_lease_agent_info(app, &args.id);
    let add_note = build_note_input(args, lease_agent.as_ref());
    let add_handoff_capsule = build_handoff_input(args, lease_agent.as_ref());
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
        gate_owner_kind: parse_gate_owner_kind_arg(args.gate_owner_kind.as_deref())?,
        gate_failure_modes: parse_gate_failure_modes_option(&args.gate_failure_modes)?,
        clear_gate_failure_modes: args.clear_gate_failure_modes,
        add_note,
        add_handoff_capsule,
        expected_profile_etag: args.if_match.clone(),
        force: args.force,
        state_actor: StateActorMetadata {
            actor_kind: args.actor_kind.clone(),
            agent_name: args.agent_name.clone(),
            agent_model: args.agent_model.clone(),
            agent_version: args.agent_version.clone(),
        },
    })
}

fn build_note_input(
    args: &crate::write_queue::UpdateOperation,
    lai: Option<&crate::domain::lease::AgentInfo>,
) -> Option<MetadataEntryInput> {
    args.add_note.clone().map(|content| MetadataEntryInput {
        content,
        username: args
            .note_username
            .clone()
            .or_else(|| lai.map(|i| i.provider.clone())),
        datetime: args.note_datetime.clone(),
        agentname: args
            .note_agentname
            .clone()
            .or_else(|| lai.map(|i| i.agent_name.clone())),
        model: args
            .note_model
            .clone()
            .or_else(|| lai.map(|i| i.model.clone())),
        version: args
            .note_version
            .clone()
            .or_else(|| lai.map(|i| i.model_version.clone())),
    })
}

fn build_handoff_input(
    args: &crate::write_queue::UpdateOperation,
    lai: Option<&crate::domain::lease::AgentInfo>,
) -> Option<MetadataEntryInput> {
    args.add_handoff_capsule
        .clone()
        .map(|content| MetadataEntryInput {
            content,
            username: args
                .handoff_username
                .clone()
                .or_else(|| lai.map(|i| i.provider.clone())),
            datetime: args.handoff_datetime.clone(),
            agentname: args
                .handoff_agentname
                .clone()
                .or_else(|| lai.map(|i| i.agent_name.clone())),
            model: args
                .handoff_model
                .clone()
                .or_else(|| lai.map(|i| i.model.clone())),
            version: args
                .handoff_version
                .clone()
                .or_else(|| lai.map(|i| i.model_version.clone())),
        })
}

pub(super) fn execute_next(app: &App, args: &NextOperation) -> Result<String, AppError> {
    let knot = app
        .show_knot(&args.id)?
        .ok_or_else(|| AppError::NotFound(args.id.clone()))?;
    validate_next_preconditions(&knot, args)?;
    let (knot, next, owner_kind) = resolve_next_state(app, &knot.id)?;
    let previous_state = knot.state.clone();
    let updated = execute_with_terminal_cascade_prompt(
        args.approve_terminal_cascade,
        |approve_terminal_cascade| {
            app.set_state_with_actor_and_options(
                &knot.id,
                &next,
                false,
                None,
                StateActorMetadata {
                    actor_kind: args.actor_kind.clone(),
                    agent_name: args.agent_name.clone(),
                    agent_model: args.agent_model.clone(),
                    agent_version: args.agent_version.clone(),
                },
                approve_terminal_cascade,
                false,
            )
        },
    )?;
    if updated.lease_id.is_some() {
        let _ = crate::lease::unbind_lease(app, &updated.id);
    }
    Ok(format_next_output(
        &updated,
        &previous_state,
        owner_kind,
        args.json,
    ))
}

fn validate_next_preconditions(
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
    validate_non_claim_lease(knot, args.lease_id.as_deref())
}

pub(super) fn execute_rollback(
    app: &App,
    args: &crate::write_queue::RollbackOperation,
) -> Result<String, AppError> {
    let resolution = resolve_rollback_state(app, &args.id)?;
    if args.dry_run {
        return Ok(format_rollback_output(
            &resolution.knot,
            &resolution.target_state,
            resolution.owner_kind,
            &resolution.reason,
            true,
        ));
    }
    let updated = app.set_state_with_actor_and_options(
        &resolution.knot.id,
        &resolution.target_state,
        resolution.requires_force,
        None,
        StateActorMetadata {
            actor_kind: args.actor_kind.clone(),
            agent_name: args.agent_name.clone(),
            agent_model: args.agent_model.clone(),
            agent_version: args.agent_version.clone(),
        },
        false,
        false,
    )?;
    Ok(format_rollback_output(
        &updated,
        &resolution.target_state,
        resolution.owner_kind,
        &resolution.reason,
        false,
    ))
}
