use crate::action_prompt;
use crate::app::{App, AppError, KnotView, StateActorMetadata};
use crate::cli::{ClaimArgs, PollArgs};
use crate::dispatch::profile_lookup_id;
use crate::domain::knot_type::KnotType;
use crate::lease_guard::validate_claim_external_lease;
use crate::prompt;
use crate::workflow::{OwnerKind, ProfileRegistry};
use crate::workflow_runtime;
use crate::write_dispatch::helpers::{
    state_actor_from_agent_info, supplied_agent_flag_names, warn_deprecated_agent_metadata,
};

#[path = "poll_claim/ready.rs"]
mod ready;
#[cfg(test)]
use crate::cli::ReadyArgs;
#[cfg(test)]
use ready::normalize_ready_type;
use ready::parse_owner_filter;
pub use ready::{list_queue_candidates, run_ready};

// Agent identity is declared by the lease, not by flags on `kno next`. The
// completion command advertised to the claiming agent only carries
// state-orienting tokens (expected state, bound lease).
const AGENT_COMPLETION_METADATA_FLAGS: &str = "--actor-kind agent";

pub struct PollResult {
    pub knot: KnotView,
    pub skill: String,
    pub completion_cmd: String,
    pub e2e: bool,
}

pub fn run_poll(app: &App, args: PollArgs) -> Result<(), AppError> {
    use crate::lease_expiry::DEFAULT_LEASE_TIMEOUT_SECONDS;
    let result = crate::trace::measure("poll_queue", || {
        poll_queue(app, args.stage.as_deref(), args.owner.as_deref(), args.e2e)
    })?;
    match result {
        None => {
            // Emit the message regardless of --json so programmatic callers
            // (e.g. quilt-worker) can distinguish "idle queue" from a real
            // error by substring-matching stderr.
            eprintln!("no claimable knots found");
            std::process::exit(1);
        }
        Some(result) => {
            if args.claim {
                let flags = supplied_agent_flag_names(
                    args.agent_name.as_deref(),
                    args.agent_model.as_deref(),
                    args.agent_version.as_deref(),
                );
                // --claim always auto-creates a lease (no external lease), so
                // no lease is bound yet at the moment of the warning.
                warn_deprecated_agent_metadata("poll --claim", &flags, false);
                let timeout = args
                    .timeout_seconds
                    .unwrap_or(DEFAULT_LEASE_TIMEOUT_SECONDS);
                let claimed = claim_knot(
                    app,
                    &result.knot.id,
                    Some("agent".to_string()),
                    None,
                    timeout,
                    args.e2e,
                )?;
                print_result(&claimed, args.json);
            } else {
                print_result(&result, args.json);
            }
        }
    }
    Ok(())
}

pub fn run_claim(app: &App, args: ClaimArgs) -> Result<(), AppError> {
    use crate::lease_expiry::DEFAULT_LEASE_TIMEOUT_SECONDS;
    // The non-peek path is routed through the write queue and warns in
    // `execute_claim`. This function also covers direct test callers and the
    // peek path, so it must warn independently.
    let flags = supplied_agent_flag_names(
        args.agent_name.as_deref(),
        args.agent_model.as_deref(),
        args.agent_version.as_deref(),
    );
    warn_deprecated_agent_metadata("claim", &flags, args.lease.is_some());
    let result = crate::trace::measure("claim", || {
        if args.peek {
            peek_knot(app, &args.id, args.e2e)
        } else {
            let timeout = args
                .timeout_seconds
                .unwrap_or(DEFAULT_LEASE_TIMEOUT_SECONDS);
            claim_knot(
                app,
                &args.id,
                Some("agent".to_string()),
                args.lease.as_deref(),
                timeout,
                args.e2e,
            )
        }
    })?;
    print_result_verbose(&result, args.json, args.verbose);
    Ok(())
}

pub fn peek_knot(app: &App, id: &str, e2e: bool) -> Result<PollResult, AppError> {
    let registry = app.profile_registry();
    let knot = app
        .show_knot(id)?
        .ok_or_else(|| AppError::NotFound(id.to_string()))?;
    require_queue_state(registry, &knot)?;
    let profile_id = profile_lookup_id(&knot);
    let next_action = workflow_runtime::next_happy_path_state(
        registry,
        &profile_id,
        knot.knot_type,
        &knot.state,
    )?
    .ok_or_else(|| {
        AppError::InvalidArgument(format!(
            "knot '{}' in state '{}' has no next action state",
            knot.id, knot.state
        ))
    })?;
    let skill = prompt_body_for_state(registry, &profile_id, &next_action)?;
    let completion_cmd = completion_command(&knot.id, &next_action, None, e2e);
    Ok(PollResult {
        knot,
        skill,
        completion_cmd,
        e2e,
    })
}

fn print_result(result: &PollResult, json: bool) {
    print_result_verbose(result, json, false);
}

fn print_result_verbose(result: &PollResult, json: bool, verbose: bool) {
    if json {
        let val = render_json_verbose(result, verbose);
        let s = serde_json::to_string_pretty(&val).expect("json serialize");
        println!("{s}");
    } else {
        print!("{}", render_text_verbose(result, verbose));
    }
}

pub fn poll_queue(
    app: &App,
    stage: Option<&str>,
    owner_filter: Option<&str>,
    e2e: bool,
) -> Result<Option<PollResult>, AppError> {
    let registry = app.profile_registry();
    let owner_kind = parse_owner_filter(owner_filter);
    let knots = list_queue_candidates(app, stage)?;
    for knot in knots {
        if let Some(result) = match_pollable(&knot, registry, &owner_kind, e2e)? {
            return Ok(Some(result));
        }
    }
    Ok(None)
}

pub fn claim_knot(
    app: &App,
    id: &str,
    actor_kind: Option<String>,
    external_lease: Option<&str>,
    timeout_seconds: u64,
    e2e: bool,
) -> Result<PollResult, AppError> {
    let registry = app.profile_registry();
    let knot = app
        .show_knot(id)?
        .ok_or_else(|| AppError::NotFound(id.to_string()))?;
    let knot = if crate::lease_guard::materialize_expired_lease(app, &knot)? {
        app.show_knot(id)?
            .ok_or_else(|| AppError::NotFound(id.to_string()))?
    } else {
        knot
    };
    require_queue_state(registry, &knot)?;
    let profile_id = profile_lookup_id(&knot);
    let next_action = workflow_runtime::next_happy_path_state(
        registry,
        &profile_id,
        knot.knot_type,
        &knot.state,
    )?
    .ok_or_else(|| {
        AppError::InvalidArgument(format!(
            "knot '{}' in state '{}' has no next action state",
            knot.id, knot.state
        ))
    })?;
    let skill = prompt_body_for_state(registry, &profile_id, &next_action)?;
    let kind = Some(actor_kind.unwrap_or_else(|| "agent".to_string()));
    if let Some(lease_id) = external_lease {
        validate_claim_external_lease(app, lease_id)?;
    }
    // Agent identity on the claim step comes from the lease that will bind to
    // this knot — the external one if provided, or the auto-created one
    // otherwise. For the auto-created path there is no agent_info source yet,
    // so agent fields stay unset.
    let claim_actor = resolve_claim_lease_actor(app, kind, external_lease);
    // Create an automatic lease before moving the work knot. A failed lease
    // creation then leaves the knot queueable. External leases have already
    // been checked above, but activation is still rechecked below.
    let prepared_lease_id = match external_lease {
        Some(lid) => lid.to_string(),
        None => create_claim_lease(app, &knot.id, timeout_seconds)?,
    };
    let auto_lease = external_lease.is_none();
    let claimed = match app.set_state_with_actor_and_options(
        &knot.id,
        &next_action,
        false,
        knot.profile_etag.as_deref(),
        claim_actor,
        false,
        true,
    ) {
        Ok(claimed) => claimed,
        Err(err) => {
            if auto_lease {
                let _ = crate::lease::terminate_lease(app, &prepared_lease_id);
            }
            return Err(err);
        }
    };

    if let Err(err) = validate_claim_external_lease(app, &prepared_lease_id) {
        compensate_failed_claim(app, &claimed.id);
        if auto_lease {
            cleanup_claim_lease(app, &prepared_lease_id);
        }
        return Err(err);
    }
    if let Err(err) = crate::lease::activate_lease(app, &prepared_lease_id) {
        compensate_failed_claim(app, &claimed.id);
        if auto_lease {
            cleanup_claim_lease(app, &prepared_lease_id);
        }
        return Err(err);
    }
    if let Err(err) = app
        .set_lease_expiry(
            &prepared_lease_id,
            crate::lease_expiry::compute_expiry_ts(timeout_seconds),
        )
        .and_then(|()| crate::lease::bind_lease(app, &claimed.id, &prepared_lease_id))
    {
        compensate_failed_claim(app, &claimed.id);
        cleanup_claim_lease(app, &prepared_lease_id);
        return Err(err);
    }

    let bound = match app.show_knot(&claimed.id)? {
        Some(bound) => bound,
        None => {
            compensate_failed_claim(app, &claimed.id);
            return Err(AppError::NotFound(claimed.id.clone()));
        }
    };
    let completion_cmd = completion_command(&bound.id, &bound.state, Some(&prepared_lease_id), e2e);
    Ok(PollResult {
        knot: bound,
        skill,
        completion_cmd,
        e2e,
    })
}

fn resolve_claim_lease_actor(
    app: &App,
    actor_kind: Option<String>,
    external_lease: Option<&str>,
) -> StateActorMetadata {
    let info = external_lease
        .and_then(|lid| app.show_knot(lid).ok().flatten())
        .and_then(|lease_knot| lease_knot.lease.clone())
        .and_then(|data| data.agent_info);
    state_actor_from_agent_info(actor_kind, info.as_ref())
}

fn create_claim_lease(app: &App, knot_id: &str, timeout_seconds: u64) -> Result<String, AppError> {
    let lease = crate::lease::create_lease(
        app,
        &format!("claim-{knot_id}"),
        crate::domain::lease::LeaseType::Agent,
        Some(crate::domain::lease::AgentInfo::default()),
        timeout_seconds,
    )?;
    Ok(lease.id)
}

fn compensate_failed_claim(app: &App, knot_id: &str) {
    let Ok(resolution) = crate::rollback::resolve_rollback_state(app, knot_id) else {
        return;
    };
    let _ = app.fail_state_transition(
        knot_id,
        &resolution.target_state,
        resolution.requires_force,
        StateActorMetadata::default(),
    );
}

fn cleanup_claim_lease(app: &App, lease_id: &str) {
    let _ = crate::lease::terminate_lease(app, lease_id);
}

pub fn render_text(result: &PollResult) -> String {
    prompt::render_prompt(
        &result.knot,
        &result.skill,
        &result.completion_cmd,
        result.e2e,
    )
}

pub fn render_text_verbose(result: &PollResult, verbose: bool) -> String {
    prompt::render_prompt_verbose(
        &result.knot,
        &result.skill,
        &result.completion_cmd,
        verbose,
        result.e2e,
    )
}

pub fn render_json(result: &PollResult) -> serde_json::Value {
    prompt::render_prompt_json(
        &result.knot,
        &result.skill,
        &result.completion_cmd,
        result.e2e,
    )
}

pub fn render_json_verbose(result: &PollResult, verbose: bool) -> serde_json::Value {
    prompt::render_prompt_json_verbose(
        &result.knot,
        &result.skill,
        &result.completion_cmd,
        verbose,
        result.e2e,
    )
}

fn match_pollable(
    knot: &KnotView,
    registry: &ProfileRegistry,
    owner_kind: &OwnerKind,
    e2e: bool,
) -> Result<Option<PollResult>, AppError> {
    let gate = knot.gate.clone().unwrap_or_default();
    let profile_id = profile_lookup_id(knot);
    let next_action = match workflow_runtime::next_happy_path_state(
        registry,
        &profile_id,
        knot.knot_type,
        &knot.state,
    )? {
        Some(s) => s,
        None => return Ok(None),
    };
    let step_owner = match workflow_runtime::owner_kind_for_state(
        registry,
        &profile_id,
        knot.knot_type,
        &gate,
        &next_action,
    )? {
        Some(o) => o,
        None => return Ok(None),
    };
    if step_owner != *owner_kind {
        return Ok(None);
    }
    let skill = match prompt_body_for_state(registry, &profile_id, &next_action) {
        Ok(skill) => skill,
        Err(_) => return Ok(None),
    };
    let completion_cmd = completion_command(&knot.id, &next_action, None, e2e);
    Ok(Some(PollResult {
        knot: knot.clone(),
        skill,
        completion_cmd,
        e2e,
    }))
}

fn require_queue_state(registry: &ProfileRegistry, knot: &KnotView) -> Result<(), AppError> {
    if knot.knot_type == KnotType::Lease {
        return Err(AppError::InvalidArgument(format!(
            "knot '{}' is a lease and cannot be claimed",
            knot.id
        )));
    }
    let profile_id = profile_lookup_id(knot);
    if !workflow_runtime::is_queue_state_for_profile(
        registry,
        &profile_id,
        knot.knot_type,
        &knot.state,
    )? {
        return Err(AppError::InvalidArgument(format!(
            "knot '{}' is in state '{}', which is not a claimable queue state",
            knot.id, knot.state
        )));
    }
    Ok(())
}

fn completion_command(
    knot_id: &str,
    current_state: &str,
    lease_id: Option<&str>,
    _e2e: bool,
) -> String {
    // The completion command drives `kno next`, which is purely a state
    // transition. E2E continuation is signalled in the workflow boundary text
    // (which tells the agent to re-claim with `--e2e`), not on the next
    // command itself.
    match lease_id {
        Some(lid) => format!(
            "kno next {knot_id} --expected-state {current_state} --lease {lid} {AGENT_COMPLETION_METADATA_FLAGS}"
        ),
        None => format!(
            "kno next {knot_id} --expected-state {current_state} {AGENT_COMPLETION_METADATA_FLAGS}"
        ),
    }
}

fn prompt_body_for_state(
    registry: &ProfileRegistry,
    profile_id: &str,
    action_state: &str,
) -> Result<String, AppError> {
    let profile = registry.require(profile_id)?;
    action_prompt::render_for_profile(profile, action_state).ok_or_else(|| {
        AppError::InvalidArgument(format!(
            "next state '{}' is not an action state with a prompt",
            action_state
        ))
    })
}

#[cfg(test)]
#[path = "poll_claim/tests_inline.rs"]
mod tests;

#[cfg(test)]
#[path = "poll_claim/tests_ext2.rs"]
mod tests_ext2;

#[cfg(test)]
#[path = "poll_claim/tests_gate_ext.rs"]
mod tests_gate_ext;

#[cfg(test)]
#[path = "poll_claim/tests_lease_ext.rs"]
mod tests_lease_ext;

#[cfg(test)]
#[path = "poll_claim/tests_lease_ext2.rs"]
mod tests_lease_ext2;
