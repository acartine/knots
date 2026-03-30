use super::{
    initial_state, is_action_state, is_action_state_for_profile, is_escape_state,
    is_queue_state, is_terminal_state, next_happy_path_state,
    owner_kind_for_state, queue_state_for_stage, validate_transition,
    EVALUATING, READY_TO_EVALUATE,
};
use crate::domain::gate::{GateData, GateOwnerKind};
use crate::domain::knot_type::KnotType;
use crate::workflow::{OwnerKind, ProfileRegistry};
use uuid::Uuid;

fn unique_workspace(prefix: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&path).expect("workspace should be creatable");
    path
}

#[test]
fn gate_states_have_explicit_queue_and_action_classification() {
    assert!(is_queue_state(READY_TO_EVALUATE));
    assert!(is_action_state(EVALUATING));
    assert!(!is_action_state(READY_TO_EVALUATE));
    assert!(!is_action_state("blocked"));
}

#[test]
fn profile_escape_states_are_non_actionable_and_non_terminal() {
    let workspace = unique_workspace("knots-workflow-escape");
    let workflow_root = workspace.join(".knots/workflows/custom_flow/1");
    std::fs::create_dir_all(&workflow_root).expect("workflow dir should exist");
    std::fs::write(
        workflow_root.join("bundle.toml"),
        r#"
[workflow]
name = "custom_flow"
version = 1
default_profile = "autopilot"

[states.ready_for_work]
kind = "queue"

[states.work]
kind = "action"
action_type = "produce"
executor = "agent"
prompt = "work"

[states.blocked]
kind = "escape"

[states.deferred]
kind = "escape"

[states.done]
kind = "terminal"

[steps.work_step]
queue = "ready_for_work"
action = "work"

[states.ready_for_review]
kind = "queue"

[states.review]
kind = "action"
action_type = "gate"
executor = "human"
prompt = "review"

[steps.review_step]
queue = "ready_for_review"
action = "review"

[phases.main]
produce = "work_step"
gate = "review_step"

[profiles.autopilot]
phases = ["main"]
output = "remote_main"

[prompts.work]
body = "Work"

[prompts.work.success]
done = "done"

[prompts.work.failure]
blocked = "blocked"

[prompts.review]
body = "Review"

[prompts.review.success]
approved = "done"

[prompts.review.failure]
changes = "ready_for_work"
"#,
    )
    .expect("bundle should write");
    std::fs::create_dir_all(workspace.join(".knots/workflows"))
        .expect(".knots workflows should exist");
    std::fs::write(
        workspace.join(".knots/workflows/current"),
        "current_workflow = \"custom_flow\"\ncurrent_version = 1\ncurrent_profile = \"autopilot\"\n",
    )
    .expect("workflow config should write");

    let registry = ProfileRegistry::load_for_repo(&workspace).expect("registry should load");
    assert!(is_escape_state(
        &registry,
        "custom_flow/autopilot",
        KnotType::Work,
        "blocked"
    )
    .expect("blocked should classify as escape"));
    assert!(!is_action_state_for_profile(
        &registry,
        "custom_flow/autopilot",
        KnotType::Work,
        "blocked"
    )
    .expect("blocked should not be actionable"));
    assert!(!is_terminal_state(
        &registry,
        "custom_flow/autopilot",
        KnotType::Work,
        "blocked"
    )
    .expect("blocked should not be terminal"));

    let _ = std::fs::remove_dir_all(workspace);
}

#[test]
fn queue_state_for_stage_maps_gate_aliases() {
    assert_eq!(queue_state_for_stage("evaluate"), Some(READY_TO_EVALUATE));
    assert_eq!(queue_state_for_stage("evaluating"), Some(READY_TO_EVALUATE));
}

#[test]
fn gate_next_happy_path_is_fixed() {
    let registry = ProfileRegistry::load().unwrap();
    assert_eq!(
        next_happy_path_state(&registry, "autopilot", KnotType::Gate, READY_TO_EVALUATE)
            .unwrap(),
        Some(EVALUATING.to_string())
    );
    assert_eq!(
        next_happy_path_state(&registry, "autopilot", KnotType::Gate, EVALUATING).unwrap(),
        Some("shipped".to_string())
    );
}

#[test]
fn gate_owner_kind_comes_from_gate_data() {
    let registry = ProfileRegistry::load().unwrap();
    let gate = GateData {
        owner_kind: GateOwnerKind::Human,
        ..Default::default()
    };
    assert_eq!(
        owner_kind_for_state(
            &registry,
            "autopilot",
            KnotType::Gate,
            &gate,
            READY_TO_EVALUATE
        )
        .unwrap(),
        Some(OwnerKind::Human)
    );
}

#[test]
fn initial_state_uses_gate_queue_for_gate_knots() {
    let registry = ProfileRegistry::load().unwrap();
    let profile = registry.require("autopilot").unwrap();
    assert_eq!(initial_state(KnotType::Gate, profile), READY_TO_EVALUATE);
}

#[test]
fn gate_terminal_state_and_transition_rules_are_fixed() {
    let registry = ProfileRegistry::load().unwrap();
    assert!(
        !is_terminal_state(&registry, "autopilot", KnotType::Gate, READY_TO_EVALUATE).unwrap()
    );
    assert!(is_terminal_state(&registry, "autopilot", KnotType::Gate, "shipped").unwrap());
    assert!(validate_transition(
        &registry,
        "autopilot",
        KnotType::Gate,
        READY_TO_EVALUATE,
        EVALUATING,
        false
    )
    .is_ok());
    assert!(validate_transition(
        &registry,
        "autopilot",
        KnotType::Gate,
        EVALUATING,
        "shipped",
        false
    )
    .is_ok());
    let err = validate_transition(
        &registry,
        "autopilot",
        KnotType::Gate,
        READY_TO_EVALUATE,
        "shipped",
        false,
    )
    .unwrap_err();
    assert!(err.to_string().contains("invalid gate transition"));
}

#[test]
fn gate_owner_and_next_state_return_none_for_terminal_states() {
    let registry = ProfileRegistry::load().unwrap();
    let gate = GateData::default();
    assert_eq!(
        owner_kind_for_state(&registry, "autopilot", KnotType::Gate, &gate, "shipped").unwrap(),
        None
    );
    assert_eq!(
        next_happy_path_state(&registry, "autopilot", KnotType::Gate, "abandoned").unwrap(),
        None
    );
}

#[test]
fn gate_transition_allows_noop_force_and_abandon() {
    let registry = ProfileRegistry::load().unwrap();
    assert!(validate_transition(
        &registry,
        "autopilot",
        KnotType::Gate,
        READY_TO_EVALUATE,
        READY_TO_EVALUATE,
        false
    )
    .is_ok());
    assert!(validate_transition(
        &registry,
        "autopilot",
        KnotType::Gate,
        READY_TO_EVALUATE,
        "abandoned",
        false
    )
    .is_ok());
    assert!(validate_transition(
        &registry,
        "autopilot",
        KnotType::Gate,
        READY_TO_EVALUATE,
        "shipped",
        true
    )
    .is_ok());
}

