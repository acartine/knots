use super::{GateMode, ProfileRegistry};

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
