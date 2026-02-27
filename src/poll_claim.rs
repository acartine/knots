use crate::app::{App, AppError, KnotView, StateActorMetadata};
use crate::cli::{ClaimArgs, PollArgs};
use crate::listing::{apply_filters, KnotListFilter};
use crate::prompt;
use crate::skills;
use crate::workflow::{OwnerKind, ProfileRegistry};

pub struct PollResult {
    pub knot: KnotView,
    pub skill: &'static str,
    pub completion_cmd: String,
}

pub fn run_poll(app: &App, args: PollArgs) -> Result<(), AppError> {
    let result = poll_queue(app, args.stage.as_deref(), args.owner.as_deref())?;
    match result {
        None => {
            if !args.json {
                eprintln!("no claimable knots found");
            }
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
                let claimed = claim_knot(app, &result.knot.id, actor)?;
                print_result(&claimed, args.json);
            } else {
                print_result(&result, args.json);
            }
        }
    }
    Ok(())
}

pub fn run_claim(app: &App, args: ClaimArgs) -> Result<(), AppError> {
    let actor = StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: args.agent_name,
        agent_model: args.agent_model,
        agent_version: args.agent_version,
    };
    let result = claim_knot(app, &args.id, actor)?;
    print_result(&result, args.json);
    Ok(())
}

fn print_result(result: &PollResult, json: bool) {
    if json {
        let val = render_json(result);
        let s = serde_json::to_string_pretty(&val).expect("json serialize");
        println!("{s}");
    } else {
        print!("{}", render_text(result));
    }
}

pub fn poll_queue(
    app: &App,
    stage: Option<&str>,
    owner_filter: Option<&str>,
) -> Result<Option<PollResult>, AppError> {
    let registry = ProfileRegistry::load()?;
    let owner_kind = parse_owner_filter(owner_filter);
    let knots = list_queue_candidates(app, stage)?;
    for knot in knots {
        if let Some(result) = match_pollable(&knot, &registry, &owner_kind)? {
            return Ok(Some(result));
        }
    }
    Ok(None)
}

pub fn claim_knot(app: &App, id: &str, actor: StateActorMetadata) -> Result<PollResult, AppError> {
    let registry = ProfileRegistry::load()?;
    let knot = app
        .show_knot(id)?
        .ok_or_else(|| AppError::NotFound(id.to_string()))?;
    let profile = registry.require(&knot.profile_id)?;
    let next_action = profile.next_happy_path_state(&knot.state).ok_or_else(|| {
        AppError::InvalidArgument(format!(
            "knot '{}' in state '{}' has no next action state",
            knot.id, knot.state
        ))
    })?;
    let skill = skills::skill_for_state(next_action).ok_or_else(|| {
        AppError::InvalidArgument(format!(
            "next state '{}' is not an action state with a skill",
            next_action
        ))
    })?;
    let claim_actor = StateActorMetadata {
        actor_kind: Some(actor.actor_kind.unwrap_or_else(|| "agent".to_string())),
        ..actor
    };
    let claimed = app.set_state_with_actor(
        &knot.id,
        next_action,
        false,
        knot.profile_etag.as_deref(),
        claim_actor,
    )?;
    let completion_cmd = format!("kno next {} --actor-kind agent", claimed.id);
    Ok(PollResult {
        knot: claimed,
        skill,
        completion_cmd,
    })
}

pub fn render_text(result: &PollResult) -> String {
    prompt::render_prompt(&result.knot, result.skill, &result.completion_cmd)
}

pub fn render_json(result: &PollResult) -> serde_json::Value {
    prompt::render_prompt_json(&result.knot, result.skill, &result.completion_cmd)
}

fn list_queue_candidates(app: &App, stage: Option<&str>) -> Result<Vec<KnotView>, AppError> {
    let state_filter = stage.map(|s| format!("ready_for_{}", s));
    let filter = KnotListFilter {
        include_all: false,
        state: state_filter,
        knot_type: None,
        profile_id: None,
        tags: Vec::new(),
        query: None,
    };
    let mut knots = apply_filters(app.list_knots()?, &filter);
    knots.retain(|k| k.state.starts_with("ready_for_"));
    knots.sort_by(|a, b| {
        let pa = a.priority.unwrap_or(i64::MAX);
        let pb = b.priority.unwrap_or(i64::MAX);
        pa.cmp(&pb).then_with(|| a.updated_at.cmp(&b.updated_at))
    });
    Ok(knots)
}

fn match_pollable(
    knot: &KnotView,
    registry: &ProfileRegistry,
    owner_kind: &OwnerKind,
) -> Result<Option<PollResult>, AppError> {
    let profile = match registry.require(&knot.profile_id) {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    let next_action = match profile.next_happy_path_state(&knot.state) {
        Some(s) => s,
        None => return Ok(None),
    };
    let step_owner = match profile.owners.for_action_state(next_action) {
        Some(o) => o,
        None => return Ok(None),
    };
    if step_owner.kind != *owner_kind {
        return Ok(None);
    }
    let skill = match skills::skill_for_state(next_action) {
        Some(s) => s,
        None => return Ok(None),
    };
    let completion_cmd = format!("kno next {} --actor-kind agent", knot.id);
    Ok(Some(PollResult {
        knot: knot.clone(),
        skill,
        completion_cmd,
    }))
}

fn parse_owner_filter(raw: Option<&str>) -> OwnerKind {
    match raw.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("human") => OwnerKind::Human,
        _ => OwnerKind::Agent,
    }
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
}
