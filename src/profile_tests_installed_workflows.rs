use super::ProfileRegistry;
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

#[test]
fn load_includes_builtin_profiles_from_all_workflow_bundles() {
    let registry = ProfileRegistry::load().expect("registry should load");
    let autopilot = registry.require("autopilot").expect("autopilot");

    assert_eq!(autopilot.workflow_id, "work_sdlc");
    assert_eq!(
        autopilot.owners.plan_review.kind,
        crate::profile::OwnerKind::Agent
    );
    assert_eq!(
        autopilot.owners.states["ready_for_plan_review"].kind,
        crate::profile::OwnerKind::Agent
    );
    assert_eq!(
        registry.require("evaluate").expect("evaluate").workflow_id,
        "gate_sdlc"
    );
    assert_eq!(
        registry.require("lease").expect("lease").workflow_id,
        "lease_sdlc"
    );
    assert_eq!(
        registry.require("explore").expect("explore").workflow_id,
        "explore_sdlc"
    );
}

#[test]
fn load_for_repo_keeps_non_work_builtin_profiles_alongside_installed_workflows() {
    let root = unique_workspace("knots-profile-installed-workflows");
    let bundle_path = root.join("custom-flow.toml");
    std::fs::write(&bundle_path, CUSTOM_BUNDLE).expect("bundle should write");
    installed_workflows::install_bundle(&root, &bundle_path).expect("bundle should install");

    let registry = ProfileRegistry::load_for_repo(&root).expect("repo registry should load");
    assert_eq!(
        registry
            .require("custom_flow/autopilot")
            .expect("custom profile")
            .workflow_id,
        "custom_flow"
    );
    assert_eq!(
        registry.require("evaluate").expect("evaluate").workflow_id,
        "gate_sdlc"
    );
    assert_eq!(
        registry.require("lease").expect("lease").workflow_id,
        "lease_sdlc"
    );
    assert_eq!(
        registry.require("explore").expect("explore").workflow_id,
        "explore_sdlc"
    );

    let _ = std::fs::remove_dir_all(root);
}
