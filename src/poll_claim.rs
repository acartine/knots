use crate::app::{App, AppError, KnotView, StateActorMetadata};
use crate::cli::{ClaimArgs, PollArgs, ReadyArgs};
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
    let result = if args.peek {
        peek_knot(app, &args.id)?
    } else {
        let actor = StateActorMetadata {
            actor_kind: Some("agent".to_string()),
            agent_name: args.agent_name,
            agent_model: args.agent_model,
            agent_version: args.agent_version,
        };
        claim_knot(app, &args.id, actor)?
    };
    print_result(&result, args.json);
    Ok(())
}

pub fn peek_knot(app: &App, id: &str) -> Result<PollResult, AppError> {
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
    let completion_cmd = format!("kno next {} --actor-kind agent", knot.id);
    Ok(PollResult {
        knot,
        skill,
        completion_cmd,
    })
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

pub fn run_ready(app: &App, args: ReadyArgs) -> Result<(), AppError> {
    let stage = normalize_ready_type(args.ready_type.as_deref());
    let candidates = list_queue_candidates(app, stage.as_deref())?;
    if args.json {
        let json =
            serde_json::to_string_pretty(&candidates).expect("JSON serialization should work");
        println!("{json}");
    } else if candidates.is_empty() {
        println!("no knots ready for action");
    } else {
        let palette = crate::ui::Palette::auto();
        for knot in &candidates {
            let sid = crate::knot_id::display_id(&knot.id);
            let display_id = knot
                .alias
                .as_deref()
                .map_or(sid.to_string(), |a| format!("{a} ({sid})"));
            println!(
                "{} {} {}",
                palette.id(&display_id),
                palette.state(&knot.state),
                knot.title
            );
        }
    }
    Ok(())
}

pub fn list_queue_candidates(app: &App, stage: Option<&str>) -> Result<Vec<KnotView>, AppError> {
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

fn normalize_ready_type(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lowered = trimmed.to_ascii_lowercase().replace('-', "_");
    if lowered.starts_with("ready_for_") {
        Some(lowered.trim_start_matches("ready_for_").to_string())
    } else {
        Some(lowered)
    }
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
    use std::path::PathBuf;

    fn unique_workspace() -> PathBuf {
        let root = std::env::temp_dir().join(format!("knots-poll-test-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&root).expect("workspace should be creatable");
        root
    }

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
    fn run_ready_empty_queue_prints_message() {
        let root = unique_workspace();
        let db_path = root.join(".knots/cache/state.sqlite");
        let app =
            App::open(db_path.to_str().expect("utf8"), root.clone()).expect("app should open");
        let args = ReadyArgs {
            ready_type: None,
            json: false,
        };
        run_ready(&app, args).expect("run_ready should succeed");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn run_ready_json_empty_queue() {
        let root = unique_workspace();
        let db_path = root.join(".knots/cache/state.sqlite");
        let app =
            App::open(db_path.to_str().expect("utf8"), root.clone()).expect("app should open");
        let args = ReadyArgs {
            ready_type: None,
            json: true,
        };
        run_ready(&app, args).expect("run_ready json should succeed");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn peek_knot_does_not_advance_state() {
        let root = unique_workspace();
        let db_path = root.join(".knots/cache/state.sqlite");
        let app =
            App::open(db_path.to_str().expect("utf8"), root.clone()).expect("app should open");
        let created = app
            .create_knot("Peek test", None, Some("work_item"), Some("default"))
            .expect("create should succeed");
        let original_state = created.state.clone();
        let result = peek_knot(&app, &created.id);
        assert!(result.is_ok(), "peek_knot should succeed");
        let after = app
            .show_knot(&created.id)
            .expect("show should succeed")
            .expect("knot should exist");
        assert_eq!(after.state, original_state, "state should be unchanged");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn run_ready_with_knot_in_queue() {
        let root = unique_workspace();
        let db_path = root.join(".knots/cache/state.sqlite");
        let app =
            App::open(db_path.to_str().expect("utf8"), root.clone()).expect("app should open");
        app.create_knot("Test ready", None, Some("work_item"), Some("default"))
            .expect("create should succeed");
        let args = ReadyArgs {
            ready_type: None,
            json: false,
        };
        run_ready(&app, args).expect("run_ready with knot should succeed");
        let _ = std::fs::remove_dir_all(root);
    }
}
