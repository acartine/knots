use std::collections::BTreeMap;

use serde_json::Value;

use super::{AppError, CreateKnotOptions, GateDecision, StateActorMetadata, UpdateKnotPatch};
use crate::domain::gate::{GateData, GateOwnerKind};
use crate::domain::invariant::{Invariant, InvariantType};

use super::tests_coverage_ext::{
    open_app, read_event_payloads, unique_workspace, CUSTOM_WORKFLOW_BUNDLE,
};

#[test]
fn set_state_actor_validation_and_deferred_resume_rules() {
    let root = unique_workspace();
    let (app, _) = open_app(&root);
    let created = app
        .create_knot("Deferred rules", None, Some("idea"), Some("default"))
        .expect("knot should be created");

    let invalid_actor = app.set_state_with_actor(
        &created.id,
        "planning",
        false,
        created.profile_etag.as_deref(),
        StateActorMetadata {
            actor_kind: Some("robot".to_string()),
            agent_name: None,
            agent_model: None,
            agent_version: None,
        },
    );
    assert!(matches!(invalid_actor, Err(AppError::InvalidArgument(_))));

    let deferred = app
        .set_state(
            &created.id,
            "deferred",
            false,
            created.profile_etag.as_deref(),
        )
        .expect("defer transition should succeed");
    assert_eq!(
        deferred.deferred_from_state.as_deref(),
        Some("ready_for_planning")
    );

    let bad_resume = app.set_state(
        &created.id,
        "ready_for_implementation",
        false,
        deferred.profile_etag.as_deref(),
    );
    assert!(matches!(bad_resume, Err(AppError::InvalidArgument(_))));

    let forced_resume = app
        .set_state(
            &created.id,
            "ready_for_implementation",
            true,
            deferred.profile_etag.as_deref(),
        )
        .expect("forced resume should succeed");
    assert_eq!(forced_resume.state, "ready_for_implementation");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn update_knot_state_change_writes_actor_metadata() {
    let root = unique_workspace();
    let (app, _) = open_app(&root);
    let created = app
        .create_knot("Update actor", None, Some("idea"), Some("default"))
        .expect("knot should be created");

    let updated = app
        .update_knot(
            &created.id,
            UpdateKnotPatch {
                title: None,
                description: None,
                acceptance: None,
                priority: None,
                status: Some("planning".to_string()),
                knot_type: None,
                add_tags: vec![],
                remove_tags: vec![],
                add_invariants: vec![],
                remove_invariants: vec![],
                clear_invariants: false,
                gate_owner_kind: None,
                gate_failure_modes: None,
                clear_gate_failure_modes: false,
                add_note: None,
                add_handoff_capsule: None,
                lease_id: None,
                expected_profile_etag: created.profile_etag.clone(),
                force: false,
                state_actor: StateActorMetadata {
                    actor_kind: Some("agent".to_string()),
                    agent_name: Some("codex".to_string()),
                    agent_model: Some("gpt-5".to_string()),
                    agent_version: Some("1".to_string()),
                },
            },
        )
        .expect("update state change should succeed");
    assert_eq!(updated.state, "planning");

    let state_events = read_event_payloads(&root, "knot.state_set");
    let event = state_events
        .iter()
        .find(|event| {
            event
                .get("data")
                .and_then(Value::as_object)
                .and_then(|value| value.get("agent_name"))
                .and_then(Value::as_str)
                == Some("codex")
        })
        .expect("update-generated state event should include actor metadata");
    assert_eq!(
        event
            .get("data")
            .and_then(Value::as_object)
            .and_then(|value| value.get("actor_kind"))
            .and_then(Value::as_str),
        Some("agent")
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn default_profile_resolution_covers_config_and_fallback_paths() {
    let root = unique_workspace();
    let (app, _) = open_app(&root);
    let app = app.with_home_override(Some(root.clone()));

    let fallback = app
        .default_profile_id()
        .expect("fallback default profile should resolve");
    assert_eq!(fallback, "autopilot");

    let config_path = root.join(".config/knots/config.toml");
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).expect("config parent should be creatable");
    }

    std::fs::write(&config_path, "not = [valid").expect("invalid config should write");
    let invalid = app.default_profile_id();
    assert!(matches!(invalid, Err(AppError::InvalidArgument(_))));

    std::fs::write(&config_path, "default_profile = \"unknown\"\n").expect("config should write");
    let unknown = app
        .default_profile_id()
        .expect("unknown configured profile should fall back");
    assert_eq!(unknown, "autopilot");

    std::fs::write(&config_path, "default_profile = \"semiauto\"\n").expect("config should write");
    let configured = app
        .default_profile_id()
        .expect("configured profile should resolve");
    assert_eq!(configured, "semiauto");

    app.set_default_profile_id("autopilot")
        .expect("repo default profile should persist without HOME");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn workflow_specific_defaults_and_create_knot_resolve_custom_workflows() {
    let root = unique_workspace();
    let bundle = root.join("custom-flow.toml");
    std::fs::write(&bundle, CUSTOM_WORKFLOW_BUNDLE).expect("bundle should write");
    crate::installed_workflows::install_bundle(&root, &bundle).expect("bundle should install");
    crate::installed_workflows::set_current_workflow_selection(&root, "custom_flow", None, None)
        .expect("workflow selection should succeed");

    let (app, _) = open_app(&root);
    assert_eq!(
        app.default_profile_id()
            .expect("default profile should resolve"),
        "custom_flow/autopilot"
    );
    assert_eq!(
        app.default_profile_id_for_workflow("custom_flow")
            .expect("workflow profile should resolve"),
        "custom_flow/autopilot"
    );

    let created = app
        .create_knot_in_workflow("Custom work", None, None, None, Some("custom_flow"))
        .expect("workflow-specific create should succeed");
    assert_eq!(created.workflow_id, "custom_flow");
    assert_eq!(created.profile_id, "custom_flow/autopilot");
    assert_eq!(created.state, "ready_for_work");

    let wrong_profile = app.create_knot_in_workflow(
        "Wrong profile",
        None,
        None,
        Some("default"),
        Some("custom_flow"),
    );
    assert!(matches!(wrong_profile, Err(AppError::InvalidArgument(_))));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn create_knot_with_namespaced_profile_uses_profile_workflow_without_explicit_workflow() {
    let root = unique_workspace();
    let bundle = root.join("custom-flow.toml");
    std::fs::write(&bundle, CUSTOM_WORKFLOW_BUNDLE).expect("bundle should write");
    crate::installed_workflows::install_bundle(&root, &bundle).expect("bundle should install");

    let (app, _) = open_app(&root);
    let created = app
        .create_knot_in_workflow(
            "Namespaced profile",
            None,
            None,
            Some("custom_flow/autopilot"),
            None,
        )
        .expect("create should resolve workflow from namespaced profile");
    assert_eq!(created.workflow_id, "custom_flow");
    assert_eq!(created.profile_id, "custom_flow/autopilot");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn default_profile_for_workflow_falls_back_to_first_available_profile() {
    let root = unique_workspace();
    let bundle = root.join("custom-flow.toml");
    let no_default_bundle = CUSTOM_WORKFLOW_BUNDLE
        .replace("default_profile = \"autopilot\"\n", "")
        .replace(
            "[profiles.autopilot]\nphases = [\"main\"]\n",
            "[profiles.beta]\nphases = [\"main\"]\n\n[profiles.alpha]\nphases = [\"main\"]\n",
        );
    std::fs::write(&bundle, no_default_bundle).expect("bundle should write");
    crate::installed_workflows::install_bundle(&root, &bundle).expect("bundle should install");
    crate::installed_workflows::set_current_workflow_selection(&root, "custom_flow", None, None)
        .expect("workflow selection should succeed");

    let (app, _) = open_app(&root);
    assert_eq!(
        app.default_profile_id_for_workflow("custom_flow")
            .expect("workflow profile should resolve"),
        "custom_flow/alpha"
    );
    assert_eq!(
        app.default_profile_id()
            .expect("default workflow profile should resolve"),
        "custom_flow/alpha"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn evaluate_gate_failure_reopens_linked_knots_and_adds_metadata() {
    let root = unique_workspace();
    let (app, _) = open_app(&root);
    let target = app
        .create_knot("Target work", None, Some("shipped"), Some("default"))
        .expect("target knot should be created");

    let mut failure_modes = BTreeMap::new();
    failure_modes.insert("release blocked".to_string(), vec![target.id.clone()]);
    let gate = app
        .create_knot_with_options(
            "Release gate",
            Some("Gate must pass before shipment"),
            None,
            Some("default"),
            None,
            CreateKnotOptions {
                knot_type: crate::domain::knot_type::KnotType::Gate,
                gate_data: GateData {
                    owner_kind: GateOwnerKind::Human,
                    failure_modes,
                },
                ..CreateKnotOptions::default()
            },
        )
        .expect("gate should be created");
    let gate = app
        .update_knot(
            &gate.id,
            UpdateKnotPatch {
                title: None,
                description: None,
                acceptance: None,
                priority: None,
                status: None,
                knot_type: None,
                add_tags: vec![],
                remove_tags: vec![],
                add_invariants: vec![Invariant::new(InvariantType::State, "release blocked")
                    .expect("invariant should build")],
                remove_invariants: vec![],
                clear_invariants: false,
                gate_owner_kind: None,
                gate_failure_modes: None,
                clear_gate_failure_modes: false,
                add_note: None,
                add_handoff_capsule: None,
                lease_id: None,
                expected_profile_etag: gate.profile_etag.clone(),
                force: false,
                state_actor: StateActorMetadata::default(),
            },
        )
        .expect("gate invariants should update");
    let gate = app
        .set_state(
            &gate.id,
            crate::workflow_runtime::EVALUATING,
            false,
            gate.profile_etag.as_deref(),
        )
        .expect("gate should enter evaluating");

    let result = app
        .evaluate_gate(
            &gate.id,
            GateDecision::No,
            Some("release blocked"),
            StateActorMetadata::default(),
        )
        .expect("gate evaluation should succeed");

    assert_eq!(result.decision, "no");
    assert_eq!(result.gate.state, "abandoned");
    assert_eq!(result.reopened, vec![target.id.clone()]);

    let reopened = app
        .show_knot(&target.id)
        .expect("show should succeed")
        .expect("target knot should exist");
    assert_eq!(reopened.state, "ready_for_planning");
    assert!(reopened
        .notes
        .last()
        .expect("note should be added")
        .content
        .contains("reopened this knot for planning"));
    assert!(reopened
        .handoff_capsules
        .last()
        .expect("handoff should be added")
        .content
        .contains("reopened this knot for planning"));

    let _ = std::fs::remove_dir_all(root);
}
