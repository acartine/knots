use std::collections::BTreeMap;

use crate::doctor::DoctorStatus;
use crate::doctor_workflows::check_registered_workflows;
use crate::domain::knot_type::KnotType;
use crate::installed_workflows::{
    ensure_builtin_workflows_registered, write_repo_config, KnotTypeWorkflowConfig, WorkflowRef,
    WorkflowRepoConfig,
};

fn unique_workspace() -> knots_test_support::TestWorkspace {
    knots_test_support::workspace("knots-doctor-workflows")
}

#[test]
fn workflow_registry_check_passes_when_builtins_are_registered() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    ensure_builtin_workflows_registered(&root).expect("builtin workflows should register");

    let check = check_registered_workflows(&root);
    assert_eq!(check.name, "workflow_registry");
    assert_eq!(check.status, DoctorStatus::Pass);
    assert!(check.detail.contains("work=work_sdlc"));
    assert!(check.detail.contains("gate=gate_sdlc"));
    assert!(check.detail.contains("lease=lease_sdlc"));
    assert!(check.detail.contains("explore=explore_sdlc"));
    assert!(check.detail.contains("execution_plan=execution_plan_sdlc"));
}

#[test]
fn workflow_registry_auto_repairs_partial_config() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let config = WorkflowRepoConfig {
        knot_type_workflows: BTreeMap::from([(
            KnotType::Work.as_str().to_string(),
            KnotTypeWorkflowConfig {
                default: WorkflowRef::new("work_sdlc", Some(1)),
                registered: vec![WorkflowRef::new("work_sdlc", Some(1))],
            },
        )]),
        default_profiles: BTreeMap::new(),
    };
    write_repo_config(&root, &config).expect("config should write");

    // Loading the registry should auto-repair by backfilling missing knot types.
    let check = check_registered_workflows(&root);
    assert_eq!(check.name, "workflow_registry");
    assert_eq!(check.status, DoctorStatus::Pass);
    assert!(check.detail.contains("gate=gate_sdlc"));
    assert!(check.detail.contains("execution_plan=execution_plan_sdlc"));
}
