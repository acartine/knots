use super::ProfileRegistry;

#[test]
fn exploration_profile_is_present_in_registry() {
    let registry = ProfileRegistry::load().expect("registry should load");
    let profile = registry
        .require("exploration")
        .expect("exploration profile should exist");
    assert_eq!(profile.id, "exploration");
    assert_eq!(profile.initial_state, "ready_for_exploration");
    assert!(profile
        .states
        .contains(&"ready_for_exploration".to_string()));
    assert!(profile.states.contains(&"exploration".to_string()));
    assert!(profile.states.contains(&"shipped".to_string()));
    assert!(profile.states.contains(&"abandoned".to_string()));
}

#[test]
fn exploration_profile_transitions() {
    let registry = ProfileRegistry::load().expect("registry should load");
    let profile = registry.require("exploration").expect("profile");
    profile
        .validate_transition("ready_for_exploration", "exploration", false)
        .expect("queue to action should be valid");
    profile
        .validate_transition("exploration", "shipped", false)
        .expect("exploration to shipped should be valid");
    profile
        .validate_transition("exploration", "abandoned", false)
        .expect("exploration to abandoned should be valid");
    profile
        .validate_transition("ready_for_exploration", "abandoned", false)
        .expect("queue to abandoned should be valid");
}

#[test]
fn exploration_profile_rejects_invalid_transitions() {
    let registry = ProfileRegistry::load().expect("registry should load");
    let profile = registry.require("exploration").expect("profile");
    let result = profile.validate_transition("ready_for_exploration", "shipped", false);
    assert!(
        result.is_err(),
        "direct transition from queue to shipped should be invalid"
    );
}

#[test]
fn exploration_profile_happy_path() {
    let registry = ProfileRegistry::load().expect("registry should load");
    let profile = registry.require("exploration").expect("profile");
    let next = profile.next_happy_path_state("ready_for_exploration");
    assert_eq!(next, Some("exploration"));
    let next = profile.next_happy_path_state("exploration");
    assert_eq!(next, Some("shipped"));
}
