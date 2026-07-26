use crate::app::{App, AppError, CreateKnotOptions};
use crate::dispatch::knot_ref;
use crate::domain::execution_plan::ExecutionPlanData;
use crate::domain::knot_type::KnotType;
use crate::domain::step_history::StepActorInfo;
use crate::poll_claim;
use crate::ui;
use crate::write_queue::{
    LeaseCreateOperation, LeaseExtendOperation, LeaseTerminateOperation, WriteOperation,
};

use super::helpers::{
    format_json, lease_state_actor, parse_gate_data_args, parse_gate_decision, parse_knot_type_arg,
    supplied_agent_flag_names, warn_deprecated_agent_metadata,
};

mod execute_failure_ops;
pub(crate) mod execute_plan_ops;
mod execute_write_ops;
mod scope_validation;

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
        WriteOperation::PlanWaveAdd(args) => execute_plan_ops::execute_wave_add(app, args),
        WriteOperation::PlanWaveRemove(args) => execute_plan_ops::execute_wave_remove(app, args),
        WriteOperation::PlanWaveMove(args) => execute_plan_ops::execute_wave_move(app, args),
        WriteOperation::PlanStepAdd(args) => execute_plan_ops::execute_step_add(app, args),
        WriteOperation::PlanStepRemove(args) => execute_plan_ops::execute_step_remove(app, args),
        WriteOperation::PlanStepMove(args) => execute_plan_ops::execute_step_move(app, args),
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
    if args.fast && args.exploration {
        return Err(AppError::InvalidArgument(
            "cannot combine -f (fast) and -e (exploration)".to_string(),
        ));
    }
    if args.exploration && args.profile.is_some() {
        return Err(AppError::InvalidArgument(
            "cannot combine -e (exploration) with --profile".to_string(),
        ));
    }
    if args.exploration && args.workflow.is_some() {
        return Err(AppError::InvalidArgument(
            "cannot combine -e (exploration) with --workflow".to_string(),
        ));
    }
    let knot_type = if args.exploration {
        KnotType::Explore
    } else {
        parse_knot_type_arg(args.knot_type.as_deref())?
    };
    let profile_override = if args.fast {
        Some(app.default_quick_profile_id()?)
    } else {
        None
    };
    let profile = profile_override.as_deref().or(args.profile.as_deref());
    let workflow = if args.fast || args.exploration {
        None
    } else {
        args.workflow.as_deref()
    };
    let gate_data = parse_gate_data_args(
        args.gate_owner_kind.as_deref(),
        &args.gate_failure_modes,
        knot_type,
    )?;
    let scope_data = scope_validation::parse_scope_patch(&args.scope)?
        .apply_to(crate::domain::scope::ScopeData::default());
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
            execution_plan_data: ExecutionPlanData {
                objective: args.objective.clone(),
                ..ExecutionPlanData::default()
            },
            scope_data,
            tags: args.tags.clone(),
            verification_steps: args.verification_steps.clone(),
            lease_id: args.lease_id.clone(),
            ..CreateKnotOptions::default()
        },
    )?;
    if args.json {
        return Ok(format_json(
            &serde_json::to_value(&knot).expect("created knot should serialize"),
        ));
    }
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
    if args.json {
        return Ok(format_json(
            &serde_json::to_value(&knot).expect("created knot should serialize"),
        ));
    }
    let palette = ui::Palette::auto();
    Ok(format!(
        "created {} {} {}\n",
        palette.id(&knot_ref(&knot)),
        palette.state(&knot.state),
        knot.title
    ))
}

fn execute_state(app: &App, args: &crate::write_queue::StateOperation) -> Result<String, AppError> {
    let lease_bound = app
        .show_knot(&args.id)
        .ok()
        .flatten()
        .map(|k| k.lease_id.is_some())
        .unwrap_or(false);
    warn_deprecated_agent_metadata(
        "state",
        &supplied_agent_flag_names(
            args.agent_name.as_deref(),
            args.agent_model.as_deref(),
            args.agent_version.as_deref(),
        ),
        lease_bound,
    );
    let actor = lease_state_actor(app, &args.id, args.actor_kind.clone());
    let knot = super::helpers::execute_with_terminal_cascade_prompt(
        args.approve_terminal_cascade,
        |approve_terminal_cascade| {
            app.set_state_with_actor_and_options(
                &args.id,
                &args.state,
                args.force,
                args.if_match.as_deref(),
                actor.clone(),
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
    warn_deprecated_agent_metadata(
        "claim",
        &supplied_agent_flag_names(
            args.agent_name.as_deref(),
            args.agent_model.as_deref(),
            args.agent_version.as_deref(),
        ),
        args.lease_id.is_some(),
    );
    let timeout = args
        .timeout_seconds
        .unwrap_or(DEFAULT_LEASE_TIMEOUT_SECONDS);
    let claimed = poll_claim::claim_knot(
        app,
        &args.id,
        Some("agent".to_string()),
        args.lease_id.as_deref(),
        timeout,
        args.e2e,
    )?;
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
    let polled =
        poll_claim::poll_queue(app, args.stage.as_deref(), args.owner.as_deref(), args.e2e)?;
    let Some(polled) = polled else {
        return Err(AppError::InvalidArgument(
            "no claimable knots found".to_string(),
        ));
    };
    warn_deprecated_agent_metadata(
        "poll --claim",
        &supplied_agent_flag_names(
            args.agent_name.as_deref(),
            args.agent_model.as_deref(),
            args.agent_version.as_deref(),
        ),
        // `poll --claim` always auto-creates a lease; no lease is bound yet
        // when this warning fires.
        false,
    );
    let timeout = args
        .timeout_seconds
        .unwrap_or(DEFAULT_LEASE_TIMEOUT_SECONDS);
    let claimed = poll_claim::claim_knot(
        app,
        &polled.knot.id,
        Some("agent".to_string()),
        None,
        timeout,
        args.e2e,
    )?;
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
    let lease_bound = app
        .show_knot(&args.id)
        .ok()
        .flatten()
        .and_then(|k| k.lease_id)
        .is_some();
    warn_deprecated_agent_metadata(
        "gate evaluate",
        &supplied_agent_flag_names(
            args.agent_name.as_deref(),
            args.agent_model.as_deref(),
            args.agent_version.as_deref(),
        ),
        lease_bound,
    );
    let actor = lease_state_actor(app, &args.id, args.actor_kind.clone());
    let result = app.evaluate_gate(
        &args.id,
        parse_gate_decision(&args.decision)?,
        args.invariant.as_deref(),
        actor,
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
    let lease_info = crate::write_dispatch::helpers::resolve_lease_agent_info(app, &args.id);
    warn_deprecated_agent_metadata(
        "step annotate",
        &supplied_agent_flag_names(
            args.agent_name.as_deref(),
            args.agent_model.as_deref(),
            args.agent_version.as_deref(),
        ),
        lease_info.is_some(),
    );
    let actor = StepActorInfo {
        actor_kind: args.actor_kind.clone(),
        agent_name: lease_info
            .as_ref()
            .map(|i| i.agent_name.clone())
            .filter(|s| !s.is_empty()),
        agent_model: lease_info
            .as_ref()
            .map(|i| i.model.clone())
            .filter(|s| !s.is_empty()),
        agent_version: lease_info
            .as_ref()
            .map(|i| i.model_version.clone())
            .filter(|s| !s.is_empty()),
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
