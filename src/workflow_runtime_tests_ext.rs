use super::{
    initial_state, is_action_state_for_profile, is_queue_state_for_profile, is_terminal_state,
    next_happy_path_state, next_outcome_state, owner_kind_for_state, queue_state_for_stage,
    validate_transition, EVALUATING, READY_TO_EVALUATE,
};
use crate::domain::gate::GateData;
use crate::domain::knot_type::KnotType;
use crate::installed_workflows;
use crate::workflow::{OwnerKind, ProfileRegistry};
use uuid::Uuid;

fn unique_workspace(prefix: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&path).expect("workspace should be creatable");
    path
}

#[test]
fn lease_initial_state_is_lease_ready() {
    let registry = ProfileRegistry::load().unwrap();
    let profile = registry.require("autopilot").unwrap();
    assert_eq!(initial_state(KnotType::Lease, profile), super::LEASE_READY);
}

#[test]
fn lease_next_happy_path_follows_lifecycle() {
    let registry = ProfileRegistry::load().unwrap();
    assert_eq!(
        next_happy_path_state(&registry, "autopilot", KnotType::Lease, super::LEASE_READY,)
            .unwrap(),
        Some(super::LEASE_ACTIVE.to_string())
    );
    assert_eq!(
        next_happy_path_state(&registry, "autopilot", KnotType::Lease, super::LEASE_ACTIVE,)
            .unwrap(),
        Some(super::LEASE_TERMINATED.to_string())
    );
    assert_eq!(
        next_happy_path_state(
            &registry,
            "autopilot",
            KnotType::Lease,
            super::LEASE_TERMINATED,
        )
        .unwrap(),
        None
    );
}

#[test]
fn lease_terminal_state_is_terminated() {
    let registry = ProfileRegistry::load().unwrap();
    assert!(is_terminal_state(
        &registry,
        "autopilot",
        KnotType::Lease,
        super::LEASE_TERMINATED,
    )
    .unwrap());
    assert!(
        !is_terminal_state(&registry, "autopilot", KnotType::Lease, super::LEASE_ACTIVE,).unwrap()
    );
}

#[test]
fn lease_owner_kind_is_always_none() {
    let registry = ProfileRegistry::load().unwrap();
    let gate = GateData::default();
    assert_eq!(
        owner_kind_for_state(
            &registry,
            "autopilot",
            KnotType::Lease,
            &gate,
            super::LEASE_ACTIVE,
        )
        .unwrap(),
        None
    );
}

#[test]
fn lease_transition_rules() {
    let registry = ProfileRegistry::load().unwrap();
    // Valid transitions
    assert!(validate_transition(
        &registry,
        "autopilot",
        KnotType::Lease,
        super::LEASE_READY,
        super::LEASE_ACTIVE,
        false,
    )
    .is_ok());
    assert!(validate_transition(
        &registry,
        "autopilot",
        KnotType::Lease,
        super::LEASE_ACTIVE,
        super::LEASE_TERMINATED,
        false,
    )
    .is_ok());
    // Direct ready -> terminated
    assert!(validate_transition(
        &registry,
        "autopilot",
        KnotType::Lease,
        super::LEASE_READY,
        super::LEASE_TERMINATED,
        false,
    )
    .is_ok());
    // Noop (same state)
    assert!(validate_transition(
        &registry,
        "autopilot",
        KnotType::Lease,
        super::LEASE_ACTIVE,
        super::LEASE_ACTIVE,
        false,
    )
    .is_ok());
    // Invalid (terminated -> active)
    let err = validate_transition(
        &registry,
        "autopilot",
        KnotType::Lease,
        super::LEASE_TERMINATED,
        super::LEASE_ACTIVE,
        false,
    )
    .unwrap_err();
    assert!(err.to_string().contains("invalid lease transition"));
    // Force overrides invalid
    assert!(validate_transition(
        &registry,
        "autopilot",
        KnotType::Lease,
        super::LEASE_TERMINATED,
        super::LEASE_ACTIVE,
        true,
    )
    .is_ok());
}

#[test]
fn work_runtime_delegates_to_profile_definition() {
    let registry = ProfileRegistry::load().unwrap();
    let gate = GateData::default();
    assert_eq!(
        initial_state(KnotType::Work, registry.require("autopilot").unwrap()),
        "ready_for_planning"
    );
    assert!(is_queue_state_for_profile(
        &registry,
        "autopilot",
        KnotType::Work,
        "ready_for_planning",
    )
    .unwrap());
    assert!(
        is_action_state_for_profile(&registry, "autopilot", KnotType::Work, "planning",).unwrap()
    );
    assert_eq!(
        next_happy_path_state(&registry, "autopilot", KnotType::Work, "planning").unwrap(),
        Some("ready_for_plan_review".to_string())
    );
    assert_eq!(
        owner_kind_for_state(
            &registry,
            "autopilot",
            KnotType::Work,
            &gate,
            "implementation"
        )
        .unwrap(),
        Some(OwnerKind::Agent)
    );
}

#[test]
fn queue_and_action_checks_report_unknown_profiles() {
    let registry = ProfileRegistry::load().unwrap();
    let err = is_queue_state_for_profile(&registry, "missing", KnotType::Work, "planning")
        .expect_err("missing profile should fail");
    assert!(err.to_string().contains("unknown profile"));
    let err = is_action_state_for_profile(&registry, "missing", KnotType::Work, "planning")
        .expect_err("missing profile should fail");
    assert!(err.to_string().contains("unknown profile"));
}

#[test]
fn gate_and_lease_queue_action_helpers_cover_remaining_paths() {
    let registry = ProfileRegistry::load().unwrap();
    assert!(
        is_queue_state_for_profile(&registry, "autopilot", KnotType::Gate, READY_TO_EVALUATE,)
            .unwrap()
    );
    assert!(is_queue_state_for_profile(
        &registry,
        "autopilot",
        KnotType::Lease,
        super::LEASE_READY,
    )
    .unwrap());
    assert!(
        is_action_state_for_profile(&registry, "autopilot", KnotType::Gate, EVALUATING,).unwrap()
    );
    assert!(is_action_state_for_profile(
        &registry,
        "autopilot",
        KnotType::Lease,
        super::LEASE_ACTIVE,
    )
    .unwrap());
    assert_eq!(queue_state_for_stage("unknown-stage"), None);
}

#[test]
fn custom_workflow_failure_outcomes_resolve_from_installed_bundle() {
    let root = unique_workspace("knots-runtime-outcomes");
    let bundle = root.join("bundle.toml");
    std::fs::write(
        &bundle,
        r#"
[workflow]
name = "custom_flow"
version = 1
default_profile = "autopilot"

[states.ready_for_work]
display_name = "Ready"
kind = "queue"

[states.work]
display_name = "Work"
kind = "action"
action_type = "produce"
executor = "agent"
prompt = "work"

[states.ready_for_review]
display_name = "Review Queue"
kind = "queue"

[states.review]
display_name = "Review"
kind = "action"
action_type = "gate"
executor = "human"
prompt = "review"

[states.deferred]
display_name = "Deferred"
kind = "escape"

[states.done]
display_name = "Done"
kind = "terminal"

[steps.work_step]
queue = "ready_for_work"
action = "work"

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
body = "Do work"

[prompts.work.success]
complete = "ready_for_review"

[prompts.work.failure]
blocked = "deferred"

[prompts.review]
body = "Review work"

[prompts.review.success]
approved = "done"

[prompts.review.failure]
changes = "ready_for_work"
"#,
    )
    .expect("bundle should write");
    installed_workflows::install_bundle(&root, &bundle).expect("bundle should install");
    installed_workflows::set_current_workflow_selection(&root, "custom_flow", Some(1), None)
        .expect("workflow selection should succeed");
    let registry = ProfileRegistry::load_for_repo(&root).expect("registry should load");

    assert_eq!(
        next_outcome_state(
            &registry,
            &root,
            "custom_flow",
            "custom_flow/autopilot",
            KnotType::Work,
            "work",
            "blocked",
        )
        .expect("outcome should resolve"),
        Some("deferred".to_string())
    );
    assert_eq!(
        next_outcome_state(
            &registry,
            &root,
            "custom_flow",
            "custom_flow/autopilot",
            KnotType::Work,
            "work",
            "success",
        )
        .expect("happy path should resolve"),
        Some("ready_for_review".to_string())
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn non_work_outcomes_return_none_without_installed_workflow_lookup() {
    let registry = ProfileRegistry::load().unwrap();
    let root = unique_workspace("knots-runtime-non-work-outcome");
    assert_eq!(
        next_outcome_state(
            &registry,
            &root,
            "custom_flow",
            "autopilot",
            KnotType::Gate,
            READY_TO_EVALUATE,
            "blocked",
        )
        .expect("gate outcomes should not error"),
        None
    );

    let _ = std::fs::remove_dir_all(root);
}
