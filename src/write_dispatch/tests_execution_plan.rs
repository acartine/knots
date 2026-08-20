use super::*;
use crate::app::App;
use crate::write_queue::{UpdateOperation, WriteOperation};

use clap::Parser;
use std::path::Path;
use std::process::Command;

fn unique_workspace(prefix: &str) -> knots_test_support::TestWorkspace {
    knots_test_support::workspace(prefix)
}

fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git").arg("-C").arg(root).args(args).output();
    let output = output.expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn setup_repo(root: &Path) {
    run_git(root, &["init"]);
    run_git(root, &["config", "user.email", "knots@example.com"]);
    run_git(root, &["config", "user.name", "Knots Test"]);
    std::fs::write(root.join("README.md"), "# knots\n").expect("readme should be writable");
    run_git(root, &["add", "README.md"]);
    run_git(root, &["commit", "-m", "init"]);
    run_git(root, &["branch", "-M", "main"]);
}

#[test]
fn operation_from_command_maps_execution_plan_file() {
    let cli = crate::cli::Cli::parse_from([
        "kno",
        "update",
        "knots-1",
        "--execution-plan-file",
        "tmp/plan.json",
    ]);
    match operation_from_command(&cli.command).unwrap() {
        WriteOperation::Update(op) => {
            let mapped = op.execution_plan_file.expect("path should map");
            assert!(
                mapped.ends_with("tmp/plan.json"),
                "unexpected mapped path: {mapped}"
            );
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn operation_from_command_maps_execution_plan_objective() {
    let cli = crate::cli::Cli::parse_from([
        "kno",
        "new",
        "Execution plan",
        "--type",
        "execution_plan",
        "--objective",
        "Coordinate rollout",
    ]);
    match operation_from_command(&cli.command).unwrap() {
        WriteOperation::New(op) => {
            assert_eq!(op.knot_type.as_deref(), Some("execution_plan"));
            assert_eq!(op.objective.as_deref(), Some("Coordinate rollout"));
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn execute_operation_update_loads_execution_plan_file() {
    let root_ws = unique_workspace("knots-wd-execution-plan-file");
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");
    let app = App::open(db.to_str().unwrap(), root.clone()).unwrap();
    let knot = app
        .create_knot("Plan carrier", None, Some("idea"), Some("default"))
        .expect("knot should be created");

    let plan_path = root.join("plan.json");
    std::fs::write(
        &plan_path,
        serde_json::json!({
            "repo_path": "/repo",
            "objective": "Ship file-based update",
            "summary": "Execution plan for file-based update",
            "knot_ids": ["knot-1"],
            "waves": [{
                "wave_index": 1,
                "name": "Persist",
                "objective": "Store the plan"
            }]
        })
        .to_string(),
    )
    .expect("plan file should write");

    let output = execute_operation(
        &app,
        &WriteOperation::Update(UpdateOperation {
            id: knot.id.clone(),
            title: None,
            description: None,
            acceptance: None,
            priority: None,
            status: None,
            knot_type: None,
            add_tags: vec![],
            remove_tags: vec![],
            add_invariants: vec![],
            remove_invariants: vec![],
            clear_invariants: false,
            add_verification_steps: vec![],
            remove_verification_steps: vec![],
            clear_verification_steps: false,
            gate_owner_kind: None,
            gate_failure_modes: vec![],
            clear_gate_failure_modes: false,
            scope: crate::cli_scope::ScopeArgs::default(),
            execution_plan_file: Some(plan_path.to_string_lossy().into_owned()),
            objective: None,
            add_note: None,
            note_username: None,
            note_datetime: None,
            note_agentname: None,
            note_model: None,
            note_version: None,
            add_handoff_capsule: None,
            handoff_username: None,
            handoff_datetime: None,
            handoff_agentname: None,
            handoff_model: None,
            handoff_version: None,
            if_match: None,
            actor_kind: None,
            agent_name: None,
            agent_model: None,
            agent_version: None,
            force: false,
            approve_terminal_cascade: false,
            lease_id: None,
            json: false,
        }),
    )
    .expect("update should succeed");
    assert!(output.contains("updated"));

    let updated = app.show_knot(&knot.id).expect("show").expect("knot exists");
    let execution_plan = updated.execution_plan.expect("payload should exist");
    assert_eq!(
        execution_plan.objective.as_deref(),
        Some("Ship file-based update")
    );
    assert_eq!(execution_plan.waves.len(), 1);
    let serialized = serde_json::to_value(&execution_plan).expect("payload should serialize");
    let plan = serialized.as_object().expect("plan should be object");
    assert_eq!(plan.get("repo_path"), None);
    assert_eq!(plan.get("knot_ids"), None);
}

#[test]
fn execute_operation_update_objective_preserves_existing_waves() {
    let root_ws = unique_workspace("knots-wd-execution-plan-objective");
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");
    let app = App::open(db.to_str().unwrap(), root.clone()).unwrap();
    let knot = app
        .create_knot("Plan carrier", None, Some("idea"), Some("default"))
        .expect("knot should be created");
    app.update_knot(
        &knot.id,
        crate::app::UpdateKnotPatch {
            execution_plan_data: Some(crate::domain::execution_plan::ExecutionPlanData {
                objective: Some("Old objective".to_string()),
                waves: vec![crate::domain::execution_plan::ExecutionPlanWave {
                    wave_index: 1,
                    name: "Wave 1".to_string(),
                    objective: "Persist".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .expect("seed plan");

    let output = execute_operation(
        &app,
        &WriteOperation::Update(UpdateOperation {
            id: knot.id.clone(),
            title: None,
            description: None,
            acceptance: None,
            priority: None,
            status: None,
            knot_type: None,
            add_tags: vec![],
            remove_tags: vec![],
            add_invariants: vec![],
            remove_invariants: vec![],
            clear_invariants: false,
            add_verification_steps: vec![],
            remove_verification_steps: vec![],
            clear_verification_steps: false,
            gate_owner_kind: None,
            gate_failure_modes: vec![],
            clear_gate_failure_modes: false,
            scope: crate::cli_scope::ScopeArgs::default(),
            execution_plan_file: None,
            objective: Some("New objective".to_string()),
            add_note: None,
            note_username: None,
            note_datetime: None,
            note_agentname: None,
            note_model: None,
            note_version: None,
            add_handoff_capsule: None,
            handoff_username: None,
            handoff_datetime: None,
            handoff_agentname: None,
            handoff_model: None,
            handoff_version: None,
            if_match: None,
            actor_kind: None,
            agent_name: None,
            agent_model: None,
            agent_version: None,
            force: false,
            approve_terminal_cascade: false,
            lease_id: None,
            json: false,
        }),
    )
    .expect("update should succeed");
    assert!(output.contains("updated"));

    let updated = app.show_knot(&knot.id).expect("show").expect("knot exists");
    let execution_plan = updated.execution_plan.expect("payload should exist");
    assert_eq!(execution_plan.objective.as_deref(), Some("New objective"));
    assert_eq!(execution_plan.waves.len(), 1);
    assert_eq!(execution_plan.waves[0].name, "Wave 1");
}
