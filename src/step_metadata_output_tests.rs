use crate::domain::gate::GateData;
use crate::domain::knot_type::KnotType;
use crate::workflow::ProfileRegistry;
use crate::workflow_runtime::step_metadata_for_state;
use uuid::Uuid;

fn unique_workspace(prefix: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&path).expect("workspace should be creatable");
    crate::installed_workflows::ensure_builtin_workflows_registered(&path)
        .expect("builtin workflows should register");
    path
}

const MULTI_OUTPUT_BUNDLE: &str = r#"
[workflow]
name = "multi_out"
version = 1
default_profile = "varied"

[states.ready_for_work]
kind = "queue"
[states.work]
kind = "action"
executor = "agent"
prompt = "work"
output = "branch"
[states.ready_for_review]
kind = "queue"
[states.review]
kind = "action"
executor = "human"
prompt = "review"
output = "approval"
[states.ready_for_shipment]
kind = "queue"
[states.shipment]
kind = "action"
executor = "agent"
prompt = "ship"
output = "remote_main"
[states.ready_for_deploy]
kind = "queue"
[states.deploy]
kind = "action"
executor = "agent"
prompt = "deploy"
[states.done]
kind = "terminal"
[states.abandoned]
kind = "terminal"

[steps.impl]
queue = "ready_for_work"
action = "work"
[steps.rev]
queue = "ready_for_review"
action = "review"
[steps.ship]
queue = "ready_for_shipment"
action = "shipment"
[steps.dep]
queue = "ready_for_deploy"
action = "deploy"

[phases.build]
produce = "impl"
gate = "rev"
[phases.release]
produce = "ship"
gate = "dep"

[profiles.varied]
phases = ["build", "release"]

[prompts.work]
accept = ["Done"]
body = "Do work."
[prompts.work.success]
complete = "ready_for_review"
[prompts.review]
accept = ["OK"]
body = "Review."
[prompts.review.success]
approved = "ready_for_shipment"
[prompts.ship]
accept = ["Shipped"]
body = "Ship it."
[prompts.ship.success]
complete = "ready_for_deploy"
[prompts.deploy]
accept = ["Deployed"]
body = "Deploy."
[prompts.deploy.success]
complete = "done"
"#;

#[test]
fn per_action_outputs_resolve_independently() {
    let workspace = unique_workspace("knots-stepmeta-multi");
    let bundle = workspace.join("multi_out.toml");
    std::fs::write(&bundle, MULTI_OUTPUT_BUNDLE).expect("write bundle");
    crate::installed_workflows::install_bundle(&workspace, &bundle).expect("install bundle");

    let registry = ProfileRegistry::load_for_repo(&workspace).expect("registry");
    let gate = GateData::default();
    let pid = "multi_out/varied";

    let work = step_metadata_for_state(&registry, pid, KnotType::Work, &gate, "work")
        .expect("resolve")
        .expect("metadata");
    assert_eq!(work.output.as_ref().unwrap().artifact_type, "branch");

    let review = step_metadata_for_state(&registry, pid, KnotType::Work, &gate, "review")
        .expect("resolve")
        .expect("metadata");
    assert_eq!(review.output.as_ref().unwrap().artifact_type, "approval");

    let shipment = step_metadata_for_state(&registry, pid, KnotType::Work, &gate, "shipment")
        .expect("resolve")
        .expect("metadata");
    assert_eq!(
        shipment.output.as_ref().unwrap().artifact_type,
        "remote_main",
    );

    // "deploy" has no output declaration, so the current projection leaves an
    // empty artifact type rather than omitting the output field entirely.
    let deploy = step_metadata_for_state(&registry, pid, KnotType::Work, &gate, "deploy")
        .expect("resolve")
        .expect("metadata");
    assert_eq!(
        deploy.output.as_ref().map(|o| o.artifact_type.as_str()),
        Some(""),
        "action without output declaration should have empty artifact_type",
    );

    // Queue state resolves through to the correct action output.
    let queue =
        step_metadata_for_state(&registry, pid, KnotType::Work, &gate, "ready_for_shipment")
            .expect("resolve")
            .expect("metadata");
    assert_eq!(queue.action_state, "shipment");
    assert_eq!(queue.output.as_ref().unwrap().artifact_type, "remote_main");

    let _ = std::fs::remove_dir_all(workspace);
}

#[test]
fn legacy_global_output_propagates_to_all_action_states() {
    // Built-in profiles use a single global output (e.g., remote_main)
    // that legacy_outputs() distributes to every action state.
    let registry = ProfileRegistry::load().expect("registry");
    let gate = GateData::default();
    let meta = step_metadata_for_state(&registry, "autopilot", KnotType::Work, &gate, "planning")
        .expect("resolve")
        .expect("metadata");
    assert_eq!(meta.action_state, "planning");
    assert_eq!(
        meta.output.as_ref().map(|o| o.artifact_type.as_str()),
        Some("remote_main"),
        "planning should inherit the global output from the profile",
    );
}
