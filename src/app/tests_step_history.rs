use super::{App, StateActorMetadata};
use crate::domain::step_history::{StepActorInfo, StepStatus};
use std::path::PathBuf;

fn unique_workspace() -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("knots-step-history-test-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace");
    root
}

fn open_app(root: &std::path::Path) -> App {
    let db = root.join(".knots/cache/state.sqlite");
    App::open(db.to_str().expect("utf8"), root.to_path_buf()).expect("app should open")
}

fn agent_actor() -> StateActorMetadata {
    StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: Some("claude-code".to_string()),
        agent_model: Some("opus-4.6".to_string()),
        agent_version: Some("0.1.0".to_string()),
    }
}

#[test]
fn claim_creates_step_record_with_agent_metadata() {
    let root = unique_workspace();
    let app = open_app(&root);
    let created = app
        .create_knot("Step test", None, None, None)
        .expect("create");
    assert!(created.step_history.is_empty());

    let claimed = app
        .set_state_with_actor(
            &created.id,
            "planning",
            false,
            created.profile_etag.as_deref(),
            agent_actor(),
        )
        .expect("claim should advance to action state");

    assert_eq!(claimed.step_history.len(), 1);
    let step = &claimed.step_history[0];
    assert_eq!(step.step, "planning");
    assert_eq!(step.phase, "action");
    assert_eq!(step.from_state, "ready_for_planning");
    assert!(step.to_state.is_none());
    assert_eq!(step.status, StepStatus::Started);
    assert_eq!(step.agent_name.as_deref(), Some("claude-code"));
    assert_eq!(step.agent_model.as_deref(), Some("opus-4.6"));
    assert_eq!(step.agent_version.as_deref(), Some("0.1.0"));
    assert!(!step.started_at.is_empty());
    assert!(step.ended_at.is_none());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn next_finalizes_active_step_record() {
    let root = unique_workspace();
    let app = open_app(&root);
    let created = app
        .create_knot("Next test", None, None, None)
        .expect("create");

    let claimed = app
        .set_state_with_actor(
            &created.id,
            "planning",
            false,
            created.profile_etag.as_deref(),
            agent_actor(),
        )
        .expect("claim");

    let advanced = app
        .set_state_with_actor(
            &created.id,
            "ready_for_plan_review",
            false,
            claimed.profile_etag.as_deref(),
            agent_actor(),
        )
        .expect("next should advance");

    assert_eq!(advanced.step_history.len(), 1);
    let step = &advanced.step_history[0];
    assert_eq!(step.status, StepStatus::Completed);
    assert_eq!(step.to_state.as_deref(), Some("ready_for_plan_review"));
    assert!(step.ended_at.is_some());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn repeated_action_review_cycles_produce_multiple_records() {
    let root = unique_workspace();
    let app = open_app(&root);
    let created = app
        .create_knot("Cycle test", None, None, None)
        .expect("create");

    // planning
    let v = app
        .set_state_with_actor(
            &created.id,
            "planning",
            false,
            created.profile_etag.as_deref(),
            agent_actor(),
        )
        .expect("to planning");

    // ready_for_plan_review
    let v = app
        .set_state_with_actor(
            &created.id,
            "ready_for_plan_review",
            false,
            v.profile_etag.as_deref(),
            agent_actor(),
        )
        .expect("to ready_for_plan_review");

    // plan_review
    let v = app
        .set_state_with_actor(
            &created.id,
            "plan_review",
            false,
            v.profile_etag.as_deref(),
            agent_actor(),
        )
        .expect("to plan_review");

    // ready_for_implementation
    let v = app
        .set_state_with_actor(
            &created.id,
            "ready_for_implementation",
            false,
            v.profile_etag.as_deref(),
            agent_actor(),
        )
        .expect("to ready_for_implementation");

    // implementation
    let v = app
        .set_state_with_actor(
            &created.id,
            "implementation",
            false,
            v.profile_etag.as_deref(),
            agent_actor(),
        )
        .expect("to implementation");

    assert_eq!(v.step_history.len(), 3);
    assert_eq!(v.step_history[0].step, "planning");
    assert_eq!(v.step_history[0].status, StepStatus::Completed);
    assert_eq!(v.step_history[1].step, "plan_review");
    assert_eq!(v.step_history[1].status, StepStatus::Completed);
    assert_eq!(v.step_history[2].step, "implementation");
    assert_eq!(v.step_history[2].status, StepStatus::Started);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn show_json_returns_step_history_field() {
    let root = unique_workspace();
    let app = open_app(&root);
    let created = app
        .create_knot("Show test", None, None, None)
        .expect("create");

    app.set_state_with_actor(
        &created.id,
        "planning",
        false,
        created.profile_etag.as_deref(),
        agent_actor(),
    )
    .expect("to planning");

    let shown = app
        .show_knot(&created.id)
        .expect("show")
        .expect("should exist");

    assert_eq!(shown.step_history.len(), 1);
    let json = serde_json::to_value(&shown).expect("serialize");
    assert!(json.get("step_history").is_some());
    let history = json["step_history"].as_array().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0]["step"], "planning");
    assert_eq!(history[0]["status"], "started");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn show_json_includes_empty_step_history_field() {
    let root = unique_workspace();
    let app = open_app(&root);
    let created = app
        .create_knot("Show empty history test", None, None, None)
        .expect("create");

    let shown = app
        .show_knot(&created.id)
        .expect("show")
        .expect("should exist");

    assert!(shown.step_history.is_empty());
    let json = serde_json::to_value(&shown).expect("serialize");
    assert!(json.get("step_history").is_some());
    assert_eq!(
        json["step_history"]
            .as_array()
            .expect("step_history array")
            .len(),
        0
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn step_annotate_changes_agent_on_active_step() {
    let root = unique_workspace();
    let app = open_app(&root);
    let created = app
        .create_knot("Annotate test", None, None, None)
        .expect("create");

    let claimed = app
        .set_state_with_actor(
            &created.id,
            "planning",
            false,
            created.profile_etag.as_deref(),
            agent_actor(),
        )
        .expect("to planning");

    assert_eq!(claimed.step_history.len(), 1);

    let new_actor = StepActorInfo {
        actor_kind: Some("agent".to_string()),
        agent_name: Some("different-agent".to_string()),
        agent_model: Some("sonnet-4.6".to_string()),
        agent_version: Some("2.0.0".to_string()),
        ..Default::default()
    };

    let annotated = app
        .step_annotate(&created.id, &new_actor)
        .expect("annotate");

    assert_eq!(annotated.step_history.len(), 2);
    let old_step = &annotated.step_history[0];
    assert_eq!(old_step.status, StepStatus::Completed);
    assert_eq!(old_step.agent_name.as_deref(), Some("claude-code"));

    let new_step = &annotated.step_history[1];
    assert_eq!(new_step.status, StepStatus::Started);
    assert_eq!(new_step.agent_name.as_deref(), Some("different-agent"));
    assert_eq!(new_step.agent_model.as_deref(), Some("sonnet-4.6"));
    assert_eq!(new_step.step, "planning");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn step_annotate_fails_when_no_active_step() {
    let root = unique_workspace();
    let app = open_app(&root);
    let created = app
        .create_knot("No active", None, None, None)
        .expect("create");

    let actor = StepActorInfo::default();
    let result = app.step_annotate(&created.id, &actor);
    assert!(result.is_err());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn step_history_persists_across_show_calls() {
    let root = unique_workspace();
    let app = open_app(&root);
    let created = app
        .create_knot("Persist test", None, None, None)
        .expect("create");

    app.set_state_with_actor(
        &created.id,
        "planning",
        false,
        created.profile_etag.as_deref(),
        agent_actor(),
    )
    .expect("to planning");

    let first = app.show_knot(&created.id).expect("show1").expect("exists");
    let second = app.show_knot(&created.id).expect("show2").expect("exists");

    assert_eq!(first.step_history, second.step_history);
    assert_eq!(first.step_history.len(), 1);

    let _ = std::fs::remove_dir_all(root);
}
