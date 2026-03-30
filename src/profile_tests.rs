use std::error::Error;

use super::{GateMode, ProfileError, ProfileRegistry};
use crate::installed_workflows;
use uuid::Uuid;

fn unique_workspace(prefix: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("temp workspace should be creatable");
    root
}

const CUSTOM_BUNDLE: &str = r#"
[workflow]
name = "custom_flow"
version = 1
default_profile = "autopilot"

[states.ready_for_work]
kind = "queue"

[states.work]
kind = "action"
executor = "agent"
prompt = "work"

[states.done]
kind = "terminal"

[states.deferred]
kind = "escape"

[states.abandoned]
kind = "terminal"

[steps.work_step]
queue = "ready_for_work"
action = "work"

[phases.main]
produce = "work_step"
gate = "work_step"

[profiles.autopilot]
phases = ["main"]

[prompts.work]
accept = ["Ship it"]
body = "Build it."

[prompts.work.success]
complete = "done"
"#;

const SECOND_BUNDLE: &str = r#"
[workflow]
name = "second_flow"
version = 1
default_profile = "autopilot"

[states.ready_for_work]
kind = "queue"

[states.work]
kind = "action"
executor = "agent"
prompt = "work"

[states.done]
kind = "terminal"

[states.deferred]
kind = "escape"

[states.abandoned]
kind = "terminal"

[steps.work_step]
queue = "ready_for_work"
action = "work"

[phases.main]
produce = "work_step"
gate = "work_step"

[profiles.autopilot]
phases = ["main"]

[prompts.work]
accept = ["Ship it"]
body = "Build it again."

[prompts.work.success]
complete = "done"
"#;

#[test]
fn loads_builtin_profiles_and_legacy_aliases() {
    let registry = ProfileRegistry::load().expect("registry should load");
    assert!(registry.require("autopilot").is_ok());
    assert!(registry.require("default").is_ok());
    assert!(registry.require("human_gate").is_ok());
}

#[test]
fn no_planning_profiles_start_at_ready_for_implementation() {
    let registry = ProfileRegistry::load().expect("registry should load");
    let profile = registry
        .require("autopilot_no_planning")
        .expect("profile should exist");
    assert_eq!(profile.initial_state, "ready_for_implementation");
    assert_eq!(profile.planning_mode, GateMode::Skipped);
    assert!(profile.states.iter().all(|state| !state.contains("plan")));
}

#[test]
fn next_happy_path_follows_sequential_states() {
    let registry = ProfileRegistry::load().expect("registry should load");
    let profile = registry.require("autopilot").expect("profile should exist");
    assert_eq!(
        profile.next_happy_path_state("ready_for_planning"),
        Some("planning")
    );
    assert_eq!(
        profile.next_happy_path_state("planning"),
        Some("ready_for_plan_review")
    );
    assert_eq!(
        profile.next_happy_path_state("shipment_review"),
        Some("shipped")
    );
}

#[test]
fn next_happy_path_returns_none_for_terminal() {
    let registry = ProfileRegistry::load().expect("registry should load");
    let profile = registry.require("autopilot").expect("profile should exist");
    assert_eq!(profile.next_happy_path_state("shipped"), None);
    assert_eq!(profile.next_happy_path_state("abandoned"), None);
}

#[test]
fn owner_for_action_state_returns_correct_owner() {
    let registry = ProfileRegistry::load().expect("registry should load");
    let autopilot = registry.require("autopilot").expect("profile should exist");
    let semiauto = registry.require("semiauto").expect("profile should exist");

    // autopilot: all agent
    let owner = autopilot.owners.for_action_state("implementation").unwrap();
    assert_eq!(owner.kind, super::OwnerKind::Agent);

    // semiauto: plan_review is human
    let owner = semiauto.owners.for_action_state("plan_review").unwrap();
    assert_eq!(owner.kind, super::OwnerKind::Human);

    // semiauto: implementation is agent
    let owner = semiauto.owners.for_action_state("implementation").unwrap();
    assert_eq!(owner.kind, super::OwnerKind::Agent);

    // non-action state returns None
    assert!(autopilot
        .owners
        .for_action_state("ready_for_planning")
        .is_none());
    assert!(autopilot.owners.for_action_state("shipped").is_none());
}

#[test]
fn owner_kind_for_state_maps_queue_and_action_states() {
    let registry = ProfileRegistry::load().expect("registry should load");
    let semiauto = registry.require("semiauto").expect("profile should exist");

    // Queue state maps to its corresponding action owner
    assert_eq!(
        semiauto
            .owners
            .owner_kind_for_state("ready_for_implementation"),
        Some(&super::OwnerKind::Agent)
    );
    // Action state returns its own owner
    assert_eq!(
        semiauto.owners.owner_kind_for_state("implementation"),
        Some(&super::OwnerKind::Agent)
    );
    // Review queue state maps to the review action owner
    assert_eq!(
        semiauto
            .owners
            .owner_kind_for_state("ready_for_plan_review"),
        Some(&super::OwnerKind::Human)
    );
    // Terminal states return None
    assert!(semiauto.owners.owner_kind_for_state("shipped").is_none());
    assert!(semiauto.owners.owner_kind_for_state("abandoned").is_none());
}

#[test]
fn load_for_repo_adds_namespaced_profiles_for_custom_workflow() {
    let root = unique_workspace("knots-profile-load-for-repo");
    let bundle_path = root.join("custom-flow.toml");
    std::fs::write(&bundle_path, CUSTOM_BUNDLE).expect("bundle should write");
    installed_workflows::install_bundle(&root, &bundle_path).expect("bundle should install");

    let registry = ProfileRegistry::load_for_repo(&root).expect("repo registry should load");
    let profile = registry
        .require("custom_flow/autopilot")
        .expect("namespaced profile should exist");
    assert_eq!(profile.workflow_id, "custom_flow");
    assert_eq!(profile.prompt_for_action_state("work"), Some("Build it."));
    assert_eq!(
        profile.acceptance_for_action_state("work"),
        &["Ship it".to_string()]
    );

    let via_alias = registry
        .require("autopilot")
        .expect("builtin autopilot should still exist");
    assert_eq!(via_alias.workflow_id, "compatibility");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn load_for_repo_adds_profiles_for_multiple_installed_workflows() {
    let root = unique_workspace("knots-profile-load-for-repo-multi");
    let first_bundle = root.join("custom-flow.toml");
    let second_bundle = root.join("second-flow.toml");
    std::fs::write(&first_bundle, CUSTOM_BUNDLE).expect("first bundle should write");
    std::fs::write(&second_bundle, SECOND_BUNDLE).expect("second bundle should write");
    installed_workflows::install_bundle(&root, &first_bundle).expect("first bundle should install");
    installed_workflows::install_bundle(&root, &second_bundle)
        .expect("second bundle should install");

    let registry = ProfileRegistry::load_for_repo(&root).expect("repo registry should load");
    assert!(registry.require("custom_flow/autopilot").is_ok());
    assert!(registry.require("second_flow/autopilot").is_ok());
    assert!(registry.require("autopilot").is_ok());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn require_state_and_transition_validation_report_unknown_states() {
    let registry = ProfileRegistry::load().expect("registry should load");
    let profile = registry.require("autopilot").expect("profile should exist");
    let err = profile
        .require_state("missing")
        .expect_err("unknown state should fail");
    assert!(err.to_string().contains("unknown state"));

    let err = profile
        .validate_transition("ready_for_planning", "shipped", false)
        .expect_err("invalid transition should fail");
    assert!(err.to_string().contains("invalid state transition"));
}

#[test]
fn profile_error_display_covers_passive_workflow_variants() {
    assert_eq!(
        ProfileError::MissingProfileReference.to_string(),
        "profile id is required"
    );
    assert_eq!(
        ProfileError::UnknownWorkflow("custom_flow".to_string()).to_string(),
        "unknown workflow 'custom_flow'"
    );
    assert_eq!(
        ProfileError::UnknownState {
            profile_id: "custom_flow/autopilot".to_string(),
            state: "blocked".to_string(),
        }
        .to_string(),
        "unknown state 'blocked' for profile 'custom_flow/autopilot'"
    );
}

#[test]
fn from_toml_rejects_empty_and_duplicate_profile_definitions() {
    let empty = ProfileRegistry::from_toml("profiles = []").expect_err("empty profile file fails");
    assert!(empty.to_string().contains("at least one profile"));

    let duplicate = ProfileRegistry::from_toml(
        r#"
            [[profiles]]
            id = "dup"
            planning_mode = "required"
            implementation_review_mode = "required"
            output = "local"

            [profiles.owners.planning]
            kind = "agent"
            [profiles.owners.plan_review]
            kind = "human"
            [profiles.owners.implementation]
            kind = "agent"
            [profiles.owners.implementation_review]
            kind = "human"
            [profiles.owners.shipment]
            kind = "agent"
            [profiles.owners.shipment_review]
            kind = "human"

            [[profiles]]
            id = "dup"
            planning_mode = "required"
            implementation_review_mode = "required"
            output = "local"

            [profiles.owners.planning]
            kind = "agent"
            [profiles.owners.plan_review]
            kind = "human"
            [profiles.owners.implementation]
            kind = "agent"
            [profiles.owners.implementation_review]
            kind = "human"
            [profiles.owners.shipment]
            kind = "agent"
            [profiles.owners.shipment_review]
            kind = "human"
        "#,
    )
    .expect_err("duplicate profiles fail");
    assert!(duplicate.to_string().contains("duplicate profile id"));
}

#[test]
fn resolve_requires_a_profile_reference() {
    let registry = ProfileRegistry::load().expect("registry should load");
    let err = registry
        .resolve(None)
        .expect_err("missing profile id should fail");
    assert!(matches!(err, ProfileError::MissingProfileReference));
}

#[test]
fn profile_error_source_covers_remaining_none_variants() {
    let invalid_bundle = ProfileError::InvalidBundle("bad bundle".to_string());
    assert!(invalid_bundle.source().is_none());

    let unknown_workflow = ProfileError::UnknownWorkflow("custom_flow".to_string());
    assert!(unknown_workflow.source().is_none());
}

#[test]
fn owner_kind_for_state_falls_back_to_canonical_queue_mapping_when_state_map_is_empty() {
    let registry = ProfileRegistry::load().expect("registry should load");
    let profile = registry.require("autopilot").expect("profile should exist");
    let mut owners = profile.owners.clone();
    owners.states.clear();

    assert_eq!(
        owners.owner_kind_for_state("ready_for_shipment"),
        Some(&super::OwnerKind::Agent)
    );
}

#[test]
fn queue_and_action_state_fallbacks_work_without_explicit_lists() {
    let registry = ProfileRegistry::load().expect("registry should load");
    let mut profile = registry
        .require("autopilot")
        .expect("profile should exist")
        .clone();
    profile.queue_states.clear();
    profile.action_states.clear();

    assert!(profile.is_queue_state("ready_to_evaluate"));
    assert!(profile.is_action_state("evaluating"));
}

#[test]
fn from_toml_supports_remote_legacy_output_mode() {
    let registry = ProfileRegistry::from_toml(
        r#"
            [[profiles]]
            id = "remote_profile"
            planning_mode = "required"
            implementation_review_mode = "required"
            output = "remote"

            [profiles.owners.planning]
            kind = "agent"
            [profiles.owners.plan_review]
            kind = "human"
            [profiles.owners.implementation]
            kind = "agent"
            [profiles.owners.implementation_review]
            kind = "human"
            [profiles.owners.shipment]
            kind = "agent"
            [profiles.owners.shipment_review]
            kind = "human"
        "#,
    )
    .expect("registry should parse");
    let profile = registry
        .require("remote_profile")
        .expect("profile should exist");
    assert_eq!(
        profile
            .outputs
            .get("implementation")
            .expect("output should exist")
            .artifact_type,
        "remote"
    );
}

#[test]
fn queue_state_and_optional_planning_transitions_are_profile_aware() {
    let registry = ProfileRegistry::load().expect("registry should load");
    let autopilot = registry.require("autopilot").expect("profile should exist");
    assert!(autopilot.is_queue_state("ready_for_planning"));
    assert_eq!(
        autopilot.action_for_queue_state("ready_for_plan_review"),
        Some("plan_review")
    );

    let optional = ProfileRegistry::from_toml(
        r#"
            [[profiles]]
            id = "test_optional_planning"
            planning_mode = "optional"
            implementation_review_mode = "required"
            output = "local"

            [profiles.owners.planning]
            kind = "agent"
            [profiles.owners.plan_review]
            kind = "human"
            [profiles.owners.implementation]
            kind = "agent"
            [profiles.owners.implementation_review]
            kind = "human"
            [profiles.owners.shipment]
            kind = "agent"
            [profiles.owners.shipment_review]
            kind = "human"
        "#,
    )
    .expect("registry should parse");
    let profile = optional
        .require("test_optional_planning")
        .expect("profile should exist");
    profile
        .validate_transition("ready_for_planning", "ready_for_implementation", false)
        .expect("optional planning should allow direct transition");
}
