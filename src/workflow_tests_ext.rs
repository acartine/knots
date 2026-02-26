use std::error::Error;

use super::{InvalidWorkflowTransition, WorkflowError, WorkflowRegistry};

fn valid_profile_toml(id: &str) -> String {
    format!(
        concat!(
            "[[profiles]]\n",
            "id = \"{id}\"\n",
            "planning_mode = \"required\"\n",
            "implementation_review_mode = \"required\"\n",
            "output = \"local\"\n",
            "owners = {{ ",
            "planning = {{ kind = \"human\" }}, ",
            "plan_review = {{ kind = \"human\" }}, ",
            "implementation = {{ kind = \"human\" }}, ",
            "implementation_review = {{ kind = \"human\" }}, ",
            "shipment = {{ kind = \"human\" }}, ",
            "shipment_review = {{ kind = \"human\" }} ",
            "}}\n",
        ),
        id = id
    )
}

#[test]
fn error_display_and_source_paths_cover_variants() {
    let transition = InvalidWorkflowTransition {
        profile_id: "autopilot".to_string(),
        from: "ready_for_planning".to_string(),
        to: "shipped".to_string(),
    };
    assert!(transition.to_string().contains("invalid state transition"));

    let toml_error: WorkflowError = toml::from_str::<toml::Value>("not =")
        .expect_err("invalid TOML should fail")
        .into();
    assert!(toml_error.to_string().contains("invalid profile TOML"));
    assert!(toml_error.source().is_some());

    let invalid = WorkflowError::InvalidDefinition("bad definition".to_string());
    assert!(invalid.to_string().contains("invalid profile definition"));
    assert!(invalid.source().is_none());

    let missing_ref = WorkflowError::MissingProfileReference;
    assert!(missing_ref.to_string().contains("profile id is required"));
    assert!(missing_ref.source().is_none());

    let unknown = WorkflowError::UnknownProfile("unknown".to_string());
    assert!(unknown.to_string().contains("unknown profile"));
    assert!(unknown.source().is_none());

    let unknown_state = WorkflowError::UnknownState {
        profile_id: "autopilot".to_string(),
        state: "unknown".to_string(),
    };
    assert!(unknown_state.to_string().contains("unknown state"));
    assert!(unknown_state.source().is_none());

    let invalid_transition: WorkflowError = transition.into();
    assert!(invalid_transition
        .to_string()
        .contains("invalid state transition"));
    assert!(invalid_transition.source().is_some());
}

#[test]
fn registry_resolve_and_require_failures_are_reported() {
    let registry = WorkflowRegistry::from_toml(&valid_profile_toml("autopilot"))
        .expect("registry should load");
    assert!(matches!(
        registry.resolve(None),
        Err(WorkflowError::MissingProfileReference)
    ));
    assert!(matches!(
        registry.resolve(Some("missing")),
        Err(WorkflowError::UnknownProfile(_))
    ));
    assert!(matches!(
        registry.require("   "),
        Err(WorkflowError::UnknownProfile(_))
    ));
}

#[test]
fn profile_definition_reports_unknown_state_and_invalid_transition() {
    let registry = WorkflowRegistry::load().expect("registry should load");
    let profile = registry
        .require("autopilot")
        .expect("autopilot profile should exist");

    assert!(matches!(
        profile.require_state("missing"),
        Err(WorkflowError::UnknownState { .. })
    ));
    assert!(profile
        .validate_transition("ready_for_planning", "planning", false)
        .is_ok());
    assert!(profile
        .validate_transition("ready_for_planning", "shipped", false)
        .is_err());
}

#[test]
fn load_rejects_empty_and_duplicate_definitions() {
    assert!(matches!(
        WorkflowRegistry::from_toml(""),
        Err(WorkflowError::InvalidDefinition(_))
    ));

    assert!(matches!(
        WorkflowRegistry::from_toml(&format!(
            "{}\n{}",
            valid_profile_toml("autopilot"),
            valid_profile_toml("autopilot")
        )),
        Err(WorkflowError::InvalidDefinition(_))
    ));
}
