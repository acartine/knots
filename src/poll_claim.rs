use crate::action_prompt;
use crate::app::{App, AppError, KnotView, StateActorMetadata};
use crate::cli::{ClaimArgs, PollArgs};
use crate::dispatch::profile_lookup_id;
use crate::domain::knot_type::KnotType;
use crate::lease_guard::validate_claim_external_lease;
use crate::prompt;
use crate::workflow::{OwnerKind, ProfileRegistry};
use crate::workflow_runtime;

#[path = "poll_claim/ready.rs"]
mod ready;
#[cfg(test)]
use crate::cli::ReadyArgs;
#[cfg(test)]
use ready::normalize_ready_type;
use ready::parse_owner_filter;
pub use ready::{list_queue_candidates, run_ready};

const AGENT_COMPLETION_METADATA_FLAGS: &str = concat!(
    "--actor-kind agent ",
    "--agent-name <AGENT_NAME> ",
    "--agent-model <AGENT_MODEL> ",
    "--agent-version <AGENT_VERSION>"
);

pub struct PollResult {
    pub knot: KnotView,
    pub skill: String,
    pub completion_cmd: String,
}

pub fn run_poll(app: &App, args: PollArgs) -> Result<(), AppError> {
    use crate::lease_expiry::DEFAULT_LEASE_TIMEOUT_SECONDS;
    let result = crate::trace::measure("poll_queue", || {
        poll_queue(app, args.stage.as_deref(), args.owner.as_deref())
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
                let actor = StateActorMetadata {
                    actor_kind: Some("agent".to_string()),
                    agent_name: args.agent_name,
                    agent_model: args.agent_model,
                    agent_version: args.agent_version,
                };
                let timeout = args
                    .timeout_seconds
                    .unwrap_or(DEFAULT_LEASE_TIMEOUT_SECONDS);
                let claimed = claim_knot(app, &result.knot.id, actor, None, timeout)?;
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
    let result = crate::trace::measure("claim", || {
        if args.peek {
            peek_knot(app, &args.id)
        } else {
            let actor = StateActorMetadata {
                actor_kind: Some("agent".to_string()),
                agent_name: args.agent_name.clone(),
                agent_model: args.agent_model.clone(),
                agent_version: args.agent_version.clone(),
            };
            let timeout = args
                .timeout_seconds
                .unwrap_or(DEFAULT_LEASE_TIMEOUT_SECONDS);
            claim_knot(app, &args.id, actor, args.lease.as_deref(), timeout)
        }
    })?;
    print_result_verbose(&result, args.json, args.verbose);
    Ok(())
}

pub fn peek_knot(app: &App, id: &str) -> Result<PollResult, AppError> {
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
    let completion_cmd = completion_command(&knot.id, &next_action, None);
    Ok(PollResult {
        knot,
        skill,
        completion_cmd,
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
) -> Result<Option<PollResult>, AppError> {
    let registry = app.profile_registry();
    let owner_kind = parse_owner_filter(owner_filter);
    let knots = list_queue_candidates(app, stage)?;
    for knot in knots {
        if let Some(result) = match_pollable(&knot, registry, &owner_kind)? {
            return Ok(Some(result));
        }
    }
    Ok(None)
}

pub fn claim_knot(
    app: &App,
    id: &str,
    actor: StateActorMetadata,
    external_lease: Option<&str>,
    timeout_seconds: u64,
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
    let claim_actor = StateActorMetadata {
        actor_kind: Some(actor.actor_kind.unwrap_or_else(|| "agent".to_string())),
        ..actor
    };
    let agent_info = build_agent_info_from_actor(&claim_actor);
    if let Some(lease_id) = external_lease {
        validate_claim_external_lease(app, lease_id)?;
    }
    let claimed = app.set_state_with_actor_and_options(
        &knot.id,
        &next_action,
        false,
        knot.profile_etag.as_deref(),
        claim_actor,
        false,
        true,
    )?;

    // Lease handling: use external lease or create a new one
    let bound_lease_id = if let Some(lid) = external_lease {
        bind_external_lease(app, &claimed.id, lid, timeout_seconds)?
    } else if let Some(info) = agent_info {
        create_and_bind_lease(app, &claimed.id, info, timeout_seconds)?
    } else {
        None
    };

    let bound = app
        .show_knot(&claimed.id)?
        .ok_or_else(|| AppError::NotFound(claimed.id.clone()))?;
    let completion_cmd = completion_command(&bound.id, &bound.state, bound_lease_id.as_deref());
    Ok(PollResult {
        knot: bound,
        skill,
        completion_cmd,
    })
}

fn bind_external_lease(
    app: &App,
    knot_id: &str,
    lid: &str,
    timeout_seconds: u64,
) -> Result<Option<String>, AppError> {
    validate_claim_external_lease(app, lid)?;
    crate::lease::activate_lease(app, lid)?;
    app.set_lease_expiry(lid, crate::lease_expiry::compute_expiry_ts(timeout_seconds))?;
    crate::lease::bind_lease(app, knot_id, lid)?;
    Ok(Some(lid.to_string()))
}

fn create_and_bind_lease(
    app: &App,
    knot_id: &str,
    info: crate::domain::lease::AgentInfo,
    timeout_seconds: u64,
) -> Result<Option<String>, AppError> {
    let lease = crate::lease::create_lease(
        app,
        &format!("claim-{}", knot_id),
        crate::domain::lease::LeaseType::Agent,
        Some(info),
        timeout_seconds,
    )?;
    crate::lease::activate_lease(app, &lease.id)?;
    crate::lease::bind_lease(app, knot_id, &lease.id)?;
    Ok(Some(lease.id))
}

pub fn render_text(result: &PollResult) -> String {
    prompt::render_prompt(&result.knot, &result.skill, &result.completion_cmd)
}

pub fn render_text_verbose(result: &PollResult, verbose: bool) -> String {
    prompt::render_prompt_verbose(&result.knot, &result.skill, &result.completion_cmd, verbose)
}

pub fn render_json(result: &PollResult) -> serde_json::Value {
    prompt::render_prompt_json(&result.knot, &result.skill, &result.completion_cmd)
}

pub fn render_json_verbose(result: &PollResult, verbose: bool) -> serde_json::Value {
    prompt::render_prompt_json_verbose(&result.knot, &result.skill, &result.completion_cmd, verbose)
}

fn match_pollable(
    knot: &KnotView,
    registry: &ProfileRegistry,
    owner_kind: &OwnerKind,
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
    let completion_cmd = completion_command(&knot.id, &next_action, None);
    Ok(Some(PollResult {
        knot: knot.clone(),
        skill,
        completion_cmd,
    }))
}

fn build_agent_info_from_actor(
    actor: &StateActorMetadata,
) -> Option<crate::domain::lease::AgentInfo> {
    let name = actor.agent_name.as_deref()?;
    Some(crate::domain::lease::AgentInfo {
        agent_type: "cli".to_string(),
        provider: String::new(),
        agent_name: name.to_string(),
        model: actor.agent_model.clone().unwrap_or_default(),
        model_version: actor.agent_version.clone().unwrap_or_default(),
    })
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

fn completion_command(knot_id: &str, current_state: &str, lease_id: Option<&str>) -> String {
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
mod tests {
    use super::*;

    #[test]
    fn parse_owner_defaults_to_agent() {
        assert_eq!(parse_owner_filter(None), OwnerKind::Agent);
        assert_eq!(parse_owner_filter(Some("")), OwnerKind::Agent);
        assert_eq!(parse_owner_filter(Some("agent")), OwnerKind::Agent);
    }

    #[test]
    fn parse_owner_recognizes_human() {
        assert_eq!(parse_owner_filter(Some("human")), OwnerKind::Human);
        assert_eq!(parse_owner_filter(Some("Human")), OwnerKind::Human);
    }

    #[test]
    fn normalize_ready_type_none_returns_none() {
        assert_eq!(normalize_ready_type(None), None);
    }

    #[test]
    fn normalize_ready_type_empty_returns_none() {
        assert_eq!(normalize_ready_type(Some("")), None);
        assert_eq!(normalize_ready_type(Some("  ")), None);
    }

    #[test]
    fn normalize_ready_type_strips_prefix() {
        assert_eq!(
            normalize_ready_type(Some("ready_for_planning")),
            Some("planning".to_string())
        );
    }

    #[test]
    fn normalize_ready_type_passes_through_stage() {
        assert_eq!(normalize_ready_type(Some("plan")), Some("plan".to_string()));
        assert_eq!(
            normalize_ready_type(Some("implementation")),
            Some("implementation".to_string())
        );
    }

    #[test]
    fn normalize_ready_type_lowercases_and_replaces_dashes() {
        assert_eq!(
            normalize_ready_type(Some("Plan-Review")),
            Some("plan_review".to_string())
        );
    }

    #[test]
    fn completion_command_includes_agent_metadata_flags() {
        let cmd = completion_command("knots-27ef", "implementation", None);
        assert_eq!(
            cmd,
            "kno next knots-27ef --expected-state implementation --actor-kind agent \
             --agent-name <AGENT_NAME> --agent-model <AGENT_MODEL> \
             --agent-version <AGENT_VERSION>"
        );
    }
}

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
