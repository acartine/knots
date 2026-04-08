use std::collections::BTreeMap;

use crate::doctor::DoctorStatus;
use crate::doctor_workflows::check_registered_workflows;
use crate::domain::knot_type::KnotType;
use crate::installed_workflows::{
    ensure_builtin_workflows_registered, write_repo_config, KnotTypeWorkflowConfig, WorkflowRef,
    WorkflowRepoConfig,
};

fn unique_workspace() -> std::path::PathBuf {
    let root =
        std::env::temp_dir().join(format!("knots-doctor-workflows-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    root
}

#[test]
fn workflow_registry_check_passes_when_builtins_are_registered() {
    let root = unique_workspace();
    ensure_builtin_workflows_registered(&root).expect("builtin workflows should register");

    let check = check_registered_workflows(&root);
    assert_eq!(check.name, "workflow_registry");
    assert_eq!(check.status, DoctorStatus::Pass);
    assert!(check.detail.contains("work=work_sdlc"));
    assert!(check.detail.contains("gate=gate_sdlc"));
    assert!(check.detail.contains("lease=lease_sdlc"));
    assert!(check.detail.contains("explore=explore_sdlc"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn workflow_registry_check_fails_when_a_knot_type_has_no_registered_workflows() {
    let root = unique_workspace();
    let config = WorkflowRepoConfig {
        knot_type_workflows: BTreeMap::from([(
            KnotType::Work.as_str().to_string(),
            KnotTypeWorkflowConfig {
                default: WorkflowRef::new("work_sdlc", Some(1)),
                registered: vec![WorkflowRef::new("work_sdlc", Some(1))],
            },
        )]),
        current_workflow: None,
        current_version: None,
        legacy_current_profile: None,
        default_profiles: BTreeMap::new(),
    };
    write_repo_config(&root, &config).expect("config should write");

    let check = check_registered_workflows(&root);
    assert_eq!(check.name, "workflow_registry");
    assert_eq!(check.status, DoctorStatus::Fail);
    assert!(check.detail.contains("gate"));

    let _ = std::fs::remove_dir_all(root);
}
