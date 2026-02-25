use std::error::Error;
use std::path::{Path, PathBuf};

use super::{
    InvalidWorkflowTransition, WorkflowDefinition, WorkflowError, WorkflowRegistry,
    WorkflowTransition,
};

fn unique_repo() -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("knots-workflow-ext-test-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(root.join(".knots")).expect("workflow test repo should be creatable");
    root
}

fn write_workflow(root: &Path, contents: &str) {
    std::fs::write(root.join(".knots").join("workflows.toml"), contents)
        .expect("workflow file should be writable");
}

fn valid_workflow_toml(id: &str) -> String {
    format!(
        concat!(
            "[[workflows]]\n",
            "id = \"{id}\"\n",
            "initial_state = \"idea\"\n",
            "states = [\"idea\", \"work_item\", \"done\"]\n",
            "terminal_states = [\"done\"]\n",
            "\n",
            "[[workflows.transitions]]\n",
            "from = \"idea\"\n",
            "to = \"work_item\"\n",
            "\n",
            "[[workflows.transitions]]\n",
            "from = \"work_item\"\n",
            "to = \"done\"\n",
        ),
        id = id
    )
}

#[test]
fn error_display_and_source_paths_cover_variants() {
    let transition = InvalidWorkflowTransition {
        workflow_id: "default".to_string(),
        from: "idea".to_string(),
        to: "shipped".to_string(),
    };
    assert!(transition.to_string().contains("invalid state transition"));

    let io_error: WorkflowError = std::io::Error::other("disk").into();
    assert!(io_error.to_string().contains("I/O error"));
    assert!(io_error.source().is_some());

    let toml_error: WorkflowError = toml::from_str::<toml::Value>("not =")
        .expect_err("invalid TOML should fail")
        .into();
    assert!(toml_error.to_string().contains("invalid workflow TOML"));
    assert!(toml_error.source().is_some());

    let missing = WorkflowError::MissingFile(PathBuf::from("/tmp/workflows.toml"));
    assert!(missing.to_string().contains("workflow file is required"));
    assert!(missing.source().is_none());

    let invalid = WorkflowError::InvalidDefinition("bad definition".to_string());
    assert!(invalid.to_string().contains("invalid workflow definition"));
    assert!(invalid.source().is_none());

    let missing_ref = WorkflowError::MissingWorkflowReference;
    assert!(missing_ref.to_string().contains("workflow id is required"));
    assert!(missing_ref.source().is_none());

    let unknown = WorkflowError::UnknownWorkflow("unknown".to_string());
    assert!(unknown.to_string().contains("unknown workflow"));
    assert!(unknown.source().is_none());

    let unknown_state = WorkflowError::UnknownState {
        workflow_id: "default".to_string(),
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
    let root = unique_repo();
    write_workflow(&root, &valid_workflow_toml("default"));

    let registry = WorkflowRegistry::load(&root).expect("registry should load");
    assert!(matches!(
        registry.resolve(None),
        Err(WorkflowError::MissingWorkflowReference)
    ));
    assert!(matches!(
        registry.resolve(Some("missing")),
        Err(WorkflowError::UnknownWorkflow(_))
    ));
    assert!(matches!(
        registry.require("   "),
        Err(WorkflowError::UnknownWorkflow(_))
    ));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn workflow_definition_reports_unknown_state_and_invalid_transition() {
    let workflow = WorkflowDefinition {
        id: "default".to_string(),
        description: Some("desc".to_string()),
        initial_state: "idea".to_string(),
        states: vec![
            "idea".to_string(),
            "work_item".to_string(),
            "done".to_string(),
        ],
        terminal_states: vec!["done".to_string()],
        transitions: vec![WorkflowTransition {
            from: "idea".to_string(),
            to: "work_item".to_string(),
        }],
    };

    assert!(matches!(
        workflow.require_state("missing"),
        Err(WorkflowError::UnknownState { .. })
    ));
    assert!(workflow
        .validate_transition("idea", "work_item", false)
        .is_ok());
    assert!(workflow
        .validate_transition("work_item", "done", false)
        .is_err());
}

#[test]
fn load_rejects_empty_and_duplicate_and_invalid_definitions() {
    let root = unique_repo();

    write_workflow(&root, "");
    assert!(matches!(
        WorkflowRegistry::load(&root),
        Err(WorkflowError::InvalidDefinition(_))
    ));

    write_workflow(
        &root,
        &format!(
            "{}\n{}",
            valid_workflow_toml("default"),
            valid_workflow_toml("default")
        ),
    );
    assert!(matches!(
        WorkflowRegistry::load(&root),
        Err(WorkflowError::InvalidDefinition(_))
    ));

    write_workflow(
        &root,
        concat!(
            "[[workflows]]\n",
            "id = \"w\"\n",
            "initial_state = \"missing\"\n",
            "states = [\"idea\"]\n",
            "terminal_states = [\"idea\"]\n",
            "\n",
            "[[workflows.transitions]]\n",
            "from = \"idea\"\n",
            "to = \"idea\"\n",
        ),
    );
    assert!(matches!(
        WorkflowRegistry::load(&root),
        Err(WorkflowError::InvalidDefinition(_))
    ));

    write_workflow(
        &root,
        concat!(
            "[[workflows]]\n",
            "id = \"w\"\n",
            "initial_state = \"idea\"\n",
            "states = [\"idea\", \"done\"]\n",
            "terminal_states = [\"missing\"]\n",
            "\n",
            "[[workflows.transitions]]\n",
            "from = \"idea\"\n",
            "to = \"done\"\n",
        ),
    );
    assert!(matches!(
        WorkflowRegistry::load(&root),
        Err(WorkflowError::InvalidDefinition(_))
    ));

    write_workflow(
        &root,
        concat!(
            "[[workflows]]\n",
            "id = \"w\"\n",
            "initial_state = \"idea\"\n",
            "states = [\"idea\", \"done\"]\n",
            "terminal_states = [\"done\"]\n",
            "\n",
            "[[workflows.transitions]]\n",
            "from = \"unknown\"\n",
            "to = \"done\"\n",
        ),
    );
    assert!(matches!(
        WorkflowRegistry::load(&root),
        Err(WorkflowError::InvalidDefinition(_))
    ));

    write_workflow(
        &root,
        concat!(
            "[[workflows]]\n",
            "id = \"w\"\n",
            "initial_state = \"idea\"\n",
            "states = [\"idea\", \"done\"]\n",
            "terminal_states = [\"done\"]\n",
            "\n",
            "[[workflows.transitions]]\n",
            "from = \"idea\"\n",
            "to = \"unknown\"\n",
        ),
    );
    assert!(matches!(
        WorkflowRegistry::load(&root),
        Err(WorkflowError::InvalidDefinition(_))
    ));

    write_workflow(
        &root,
        concat!(
            "[[workflows]]\n",
            "id = \"w\"\n",
            "initial_state = \"idea\"\n",
            "states = [\"idea\", \"done\"]\n",
            "terminal_states = [\"done\"]\n",
            "transitions = []\n",
        ),
    );
    assert!(matches!(
        WorkflowRegistry::load(&root),
        Err(WorkflowError::InvalidDefinition(_))
    ));

    let _ = std::fs::remove_dir_all(root);
}
