use crate::app::{App, AppError, CreateKnotOptions, StateActorMetadata};
use crate::dispatch::knot_ref;
use crate::domain::step_history::StepActorInfo;
use crate::poll_claim;
use crate::ui;
use crate::write_queue::{
    LeaseCreateOperation, LeaseExtendOperation, LeaseTerminateOperation, WriteOperation,
};

use super::helpers::{
    format_json, parse_gate_data_args, parse_gate_decision, parse_knot_type_arg,
    reject_non_claim_lease_binding,
};

mod execute_write_ops;

pub(crate) fn execute_operation(app: &App, operation: &WriteOperation) -> Result<String, AppError> {
    match operation {
        WriteOperation::New(args) => execute_new(app, args),
        WriteOperation::QuickNew(args) => execute_quick_new(app, args),
        WriteOperation::State(args) => execute_state(app, args),
        WriteOperation::Update(args) => execute_write_ops::execute_update(app, args),
        WriteOperation::Next(args) => execute_write_ops::execute_next(app, args),
        WriteOperation::Rollback(args) => execute_write_ops::execute_rollback(app, args),
        WriteOperation::Claim(args) => execute_claim(app, args),
        WriteOperation::PollClaim(args) => execute_poll_claim(app, args),
        WriteOperation::GateEvaluate(args) => execute_gate_evaluate(app, args),
        WriteOperation::EdgeAdd(args) => {
            let edge = app.add_edge(&args.src, &args.kind, &args.dst)?;
            Ok(format!(
                "edge added: {} -[{}]-> {}\n",
                edge.src, edge.kind, edge.dst
            ))
        }
        WriteOperation::EdgeRemove(args) => {
            let edge = app.remove_edge(&args.src, &args.kind, &args.dst)?;
            Ok(format!(
                "edge removed: {} -[{}]-> {}\n",
                edge.src, edge.kind, edge.dst
            ))
        }
        WriteOperation::StepAnnotate(args) => execute_step_annotate(app, args),
        WriteOperation::LeaseCreate(op) => execute_lease_create(app, op),
        WriteOperation::LeaseTerminate(op) => execute_lease_terminate(app, op),
        WriteOperation::LeaseExtend(op) => execute_lease_extend(app, op),
    }
}

fn execute_new(app: &App, args: &crate::write_queue::NewOperation) -> Result<String, AppError> {
    reject_non_claim_lease_binding(args.lease_id.as_deref())?;
    let profile_override = if args.fast {
        Some(app.default_quick_profile_id()?)
    } else {
        None
    };
    let profile = profile_override.as_deref().or(args.profile.as_deref());
    let workflow = if args.fast {
        None
    } else {
        args.workflow.as_deref()
    };
    let knot_type = parse_knot_type_arg(args.knot_type.as_deref())?;
    let gate_data = parse_gate_data_args(
        args.gate_owner_kind.as_deref(),
        &args.gate_failure_modes,
        knot_type,
    )?;
    let knot = app.create_knot_with_options(
        &args.title,
        args.description.as_deref(),
        args.state.as_deref(),
        profile,
        workflow,
        CreateKnotOptions {
            acceptance: args.acceptance.clone(),
            knot_type,
            gate_data,
            ..CreateKnotOptions::default()
        },
    )?;
    let palette = ui::Palette::auto();
    Ok(format!(
        "created {} {} {}\n",
        palette.id(&knot_ref(&knot)),
        palette.state(&knot.state),
        knot.title
    ))
}

fn execute_quick_new(
    app: &App,
    args: &crate::write_queue::QuickNewOperation,
) -> Result<String, AppError> {
    let quick_profile = app.default_quick_profile_id()?;
    let knot = app.create_knot(
        &args.title,
        args.description.as_deref(),
        args.state.as_deref(),
        Some(&quick_profile),
    )?;
    let palette = ui::Palette::auto();
    Ok(format!(
        "created {} {} {}\n",
        palette.id(&knot_ref(&knot)),
        palette.state(&knot.state),
        knot.title
    ))
}

fn execute_state(app: &App, args: &crate::write_queue::StateOperation) -> Result<String, AppError> {
    let knot = super::helpers::execute_with_terminal_cascade_prompt(
        args.approve_terminal_cascade,
        |approve_terminal_cascade| {
            app.set_state_with_actor_and_options(
                &args.id,
                &args.state,
                args.force,
                args.if_match.as_deref(),
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
    let palette = ui::Palette::auto();
    Ok(format!(
        "updated {} -> {}\n",
        palette.id(&knot_ref(&knot)),
        palette.state(&knot.state)
    ))
}

fn execute_claim(app: &App, args: &crate::write_queue::ClaimOperation) -> Result<String, AppError> {
    use crate::lease_expiry::DEFAULT_LEASE_TIMEOUT_SECONDS;
    let actor = StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: args.agent_name.clone(),
        agent_model: args.agent_model.clone(),
        agent_version: args.agent_version.clone(),
    };
    let timeout = args
        .timeout_seconds
        .unwrap_or(DEFAULT_LEASE_TIMEOUT_SECONDS);
    let claimed = poll_claim::claim_knot(app, &args.id, actor, args.lease_id.as_deref(), timeout)?;
    if args.json {
        let value = poll_claim::render_json_verbose(&claimed, args.verbose);
        Ok(format_json(&value))
    } else {
        Ok(poll_claim::render_text_verbose(&claimed, args.verbose))
    }
}

fn execute_poll_claim(
    app: &App,
    args: &crate::write_queue::PollClaimOperation,
) -> Result<String, AppError> {
    use crate::lease_expiry::DEFAULT_LEASE_TIMEOUT_SECONDS;
    let polled = poll_claim::poll_queue(app, args.stage.as_deref(), args.owner.as_deref())?;
    let Some(polled) = polled else {
        return Err(AppError::InvalidArgument(
            "no claimable knots found".to_string(),
        ));
    };
    let actor = StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: args.agent_name.clone(),
        agent_model: args.agent_model.clone(),
        agent_version: args.agent_version.clone(),
    };
    let timeout = args
        .timeout_seconds
        .unwrap_or(DEFAULT_LEASE_TIMEOUT_SECONDS);
    let claimed = poll_claim::claim_knot(app, &polled.knot.id, actor, None, timeout)?;
    if args.json {
        let value = poll_claim::render_json(&claimed);
        Ok(format_json(&value))
    } else {
        Ok(poll_claim::render_text(&claimed))
    }
}

fn execute_gate_evaluate(
    app: &App,
    args: &crate::write_queue::GateEvaluateOperation,
) -> Result<String, AppError> {
    let result = app.evaluate_gate(
        &args.id,
        parse_gate_decision(&args.decision)?,
        args.invariant.as_deref(),
        StateActorMetadata {
            actor_kind: args.actor_kind.clone(),
            agent_name: args.agent_name.clone(),
            agent_model: args.agent_model.clone(),
            agent_version: args.agent_version.clone(),
        },
    )?;
    if args.json {
        Ok(format_json(
            &serde_json::to_value(&result).expect("gate evaluation should serialize"),
        ))
    } else {
        let palette = ui::Palette::auto();
        let reopened = if result.reopened.is_empty() {
            String::new()
        } else {
            format!(" reopened={}", result.reopened.len())
        };
        Ok(format!(
            "evaluated {} -> {} decision={}{}\n",
            palette.id(&knot_ref(&result.gate)),
            palette.state(&result.gate.state),
            result.decision,
            reopened
        ))
    }
}

fn execute_step_annotate(
    app: &App,
    args: &crate::write_queue::StepAnnotateOperation,
) -> Result<String, AppError> {
    let actor = StepActorInfo {
        actor_kind: args.actor_kind.clone(),
        agent_name: args.agent_name.clone(),
        agent_model: args.agent_model.clone(),
        agent_version: args.agent_version.clone(),
        ..Default::default()
    };
    let knot = app.step_annotate(&args.id, &actor)?;
    if args.json {
        let result = serde_json::json!({
            "id": &knot.id,
            "state": &knot.state,
            "step_history": &knot.step_history,
        });
        Ok(format_json(&result))
    } else {
        let palette = ui::Palette::auto();
        Ok(format!("step annotated {}\n", palette.id(&knot_ref(&knot))))
    }
}

fn execute_lease_create(app: &App, op: &LeaseCreateOperation) -> Result<String, AppError> {
    use crate::domain::lease::{AgentInfo, LeaseType};
    use crate::lease_expiry::DEFAULT_LEASE_TIMEOUT_SECONDS;
    let lease_type = match op.lease_type.as_str() {
        "manual" => LeaseType::Manual,
        _ => LeaseType::Agent,
    };
    let agent_info = if lease_type == LeaseType::Agent {
        Some(AgentInfo {
            agent_type: op.agent_type.clone().unwrap_or_default(),
            provider: op.provider.clone().unwrap_or_default(),
            agent_name: op.agent_name.clone().unwrap_or_default(),
            model: op.model.clone().unwrap_or_default(),
            model_version: op.model_version.clone().unwrap_or_default(),
        })
    } else {
        None
    };
    let timeout = op.timeout_seconds.unwrap_or(DEFAULT_LEASE_TIMEOUT_SECONDS);
    let view = crate::lease::create_lease(app, &op.nickname, lease_type, agent_info, timeout)?;
    if op.json {
        Ok(format_json(
            &serde_json::to_value(&view).expect("serialize"),
        ))
    } else {
        let palette = ui::Palette::auto();
        Ok(format!(
            "created lease {} {}\n",
            palette.id(&knot_ref(&view)),
            view.title,
        ))
    }
}

fn execute_lease_terminate(app: &App, op: &LeaseTerminateOperation) -> Result<String, AppError> {
    let view = crate::lease::terminate_lease(app, &op.id)?;
    let palette = ui::Palette::auto();
    Ok(format!(
        "terminated lease {} -> {}\n",
        palette.id(&knot_ref(&view)),
        palette.state(&view.state),
    ))
}

fn execute_lease_extend(app: &App, args: &LeaseExtendOperation) -> Result<String, AppError> {
    let lease = app
        .show_knot(&args.lease_id)?
        .ok_or_else(|| AppError::NotFound(args.lease_id.clone()))?;
    if lease.knot_type != crate::domain::knot_type::KnotType::Lease {
        return Err(AppError::InvalidArgument(
            "specified knot is not a lease".to_string(),
        ));
    }
    let effective = crate::lease_expiry::effective_lease_state(&lease.state, lease.lease_expiry_ts);
    if effective == crate::workflow_runtime::LEASE_TERMINATED {
        return Err(AppError::InvalidArgument(
            "cannot extend a terminated or expired lease".to_string(),
        ));
    }
    let timeout = args
        .timeout_seconds
        .unwrap_or(crate::lease_expiry::DEFAULT_LEASE_TIMEOUT_SECONDS);
    let new_ts = crate::lease_expiry::compute_expiry_ts(timeout);
    app.set_lease_expiry(&args.lease_id, new_ts)?;
    let palette = crate::ui::Palette::auto();
    if args.json {
        Ok(serde_json::json!({
            "id": args.lease_id,
            "lease_expiry_ts": new_ts,
            "timeout_seconds": timeout,
        })
        .to_string()
            + "\n")
    } else {
        Ok(format!(
            "extended {} for {}s\n",
            palette.id(&args.lease_id),
            timeout
        ))
    }
}
