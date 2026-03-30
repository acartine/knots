use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Parser;
use uuid::Uuid;

use super::{
    execute_operation, operation_from_command, parse_gate_data_args, parse_gate_decision,
    parse_gate_failure_modes_option, parse_gate_owner_kind_arg, parse_knot_type_arg,
};
use crate::app::{App, CreateKnotOptions, StateActorMetadata, UpdateKnotPatch};
use crate::domain::gate::{GateData, GateOwnerKind};
use crate::domain::invariant::{Invariant, InvariantType};
use crate::domain::knot_type::KnotType;
use crate::write_queue::{GateEvaluateOperation, RollbackOperation, WriteOperation};

fn unique_workspace(prefix: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("temp workspace should be creatable");
    root
}

fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git command should run");
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

fn open_app(root: &Path) -> App {
    let db_path = root.join(".knots/cache/state.sqlite");
    App::open(db_path.to_str().expect("utf8 db path"), root.to_path_buf()).expect("app should open")
}

#[test]
fn gate_parse_helpers_cover_valid_and_invalid_inputs() {
    assert!(matches!(
        parse_gate_decision("yes").expect("yes should parse"),
        crate::app::GateDecision::Yes
    ));
    assert!(parse_gate_decision("maybe").is_err());

    assert_eq!(
        parse_knot_type_arg(None).expect("default knot type should parse"),
        KnotType::Work
    );
    assert!(parse_knot_type_arg(Some("wat")).is_err());

    assert_eq!(
        parse_gate_owner_kind_arg(Some("human")).expect("owner kind should parse"),
        Some(GateOwnerKind::Human)
    );
    assert!(parse_gate_owner_kind_arg(Some("robot")).is_err());

    assert!(parse_gate_failure_modes_option(&[])
        .expect("empty modes should parse")
        .is_none());
    assert!(parse_gate_failure_modes_option(&["missing".to_string()]).is_err());

    let gate_data = parse_gate_data_args(
        Some("human"),
        &["release blocked=knots-1,knots-2".to_string()],
        KnotType::Gate,
    )
    .expect("gate metadata should parse");
    assert_eq!(gate_data.owner_kind, GateOwnerKind::Human);
    assert_eq!(
        gate_data.failure_modes.get("release blocked"),
        Some(&vec!["knots-1".to_string(), "knots-2".to_string()])
    );

    let err = parse_gate_data_args(Some("human"), &[], KnotType::Work)
        .expect_err("work knots should reject gate metadata");
    assert!(err.to_string().contains("require knot type 'gate'"));
}

#[test]
fn operation_from_command_maps_gate_specific_arguments() {
    let new_cli = crate::cli::Cli::parse_from([
        "kno",
        "new",
        "Release gate",
        "--type",
        "gate",
        "--gate-owner-kind",
        "human",
        "--gate-failure-mode",
        "release blocked=knots-1",
    ]);
    match operation_from_command(&new_cli.command).expect("new should queue") {
        WriteOperation::New(operation) => {
            assert_eq!(operation.knot_type.as_deref(), Some("gate"));
            assert_eq!(operation.gate_owner_kind.as_deref(), Some("human"));
            assert_eq!(
                operation.gate_failure_modes,
                vec!["release blocked=knots-1".to_string()]
            );
        }
        other => panic!("unexpected new operation: {other:?}"),
    }

    let update_cli = crate::cli::Cli::parse_from([
        "kno",
        "update",
        "knots-1",
        "--gate-owner-kind",
        "agent",
        "--clear-gate-failure-modes",
    ]);
    match operation_from_command(&update_cli.command).expect("update should queue") {
        WriteOperation::Update(operation) => {
            assert_eq!(operation.gate_owner_kind.as_deref(), Some("agent"));
            assert!(operation.clear_gate_failure_modes);
        }
        other => panic!("unexpected update operation: {other:?}"),
    }

    let gate_cli = crate::cli::Cli::parse_from([
        "kno",
        "gate",
        "evaluate",
        "knots-1",
        "--decision",
        "no",
        "--invariant",
        "release blocked",
        "--json",
    ]);
    match operation_from_command(&gate_cli.command).expect("gate evaluate should queue") {
        WriteOperation::GateEvaluate(operation) => {
            assert_eq!(operation.id, "knots-1");
            assert_eq!(operation.decision, "no");
            assert_eq!(operation.invariant.as_deref(), Some("release blocked"));
            assert!(operation.json);
        }
        other => panic!("unexpected gate evaluate operation: {other:?}"),
    }
}

#[test]
fn execute_operation_gate_evaluate_covers_text_and_json_output() {
    let root = unique_workspace("knots-write-dispatch-gate-ext");
    setup_repo(&root);
    let app = open_app(&root);

    let gate = app
        .create_knot_with_options(
            "Ship gate",
            None,
            None,
            None,
            None,
            CreateKnotOptions {
                knot_type: KnotType::Gate,
                gate_data: GateData::default(),
                ..CreateKnotOptions::default()
            },
        )
        .expect("gate should be created");
    let gate = app
        .set_state(
            &gate.id,
            crate::workflow_runtime::EVALUATING,
            false,
            gate.profile_etag.as_deref(),
        )
        .expect("gate should enter evaluating");
    let text = execute_operation(
        &app,
        &WriteOperation::GateEvaluate(GateEvaluateOperation {
            id: gate.id.clone(),
            decision: "yes".to_string(),
            invariant: None,
            json: false,
            actor_kind: None,
            agent_name: None,
            agent_model: None,
            agent_version: None,
        }),
    )
    .expect("text evaluation should succeed");
    assert!(text.contains("evaluated"));
    assert!(text.contains("decision=yes"));

    let target = app
        .create_knot("Blocked work", None, Some("shipped"), None)
        .expect("target should be created");
    let mut failure_modes = BTreeMap::new();
    failure_modes.insert("release blocked".to_string(), vec![target.id.clone()]);
    let gate = app
        .create_knot_with_options(
            "Fail gate",
            None,
            None,
            None,
            None,
            CreateKnotOptions {
                knot_type: KnotType::Gate,
                gate_data: GateData {
                    owner_kind: GateOwnerKind::Agent,
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
                expected_profile_etag: gate.profile_etag.clone(),
                force: false,
                state_actor: StateActorMetadata::default(),
            },
        )
        .expect("gate should update");
    let gate = app
        .set_state(
            &gate.id,
            crate::workflow_runtime::EVALUATING,
            false,
            gate.profile_etag.as_deref(),
        )
        .expect("gate should enter evaluating");
    let json = execute_operation(
        &app,
        &WriteOperation::GateEvaluate(GateEvaluateOperation {
            id: gate.id,
            decision: "no".to_string(),
            invariant: Some("release blocked".to_string()),
            json: true,
            actor_kind: None,
            agent_name: None,
            agent_model: None,
            agent_version: None,
        }),
    )
    .expect("json evaluation should succeed");
    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("json evaluation output should parse");
    assert_eq!(parsed["decision"], "no");
    assert_eq!(parsed["gate"]["state"], "abandoned");
    assert_eq!(parsed["reopened"][0], target.id);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn execute_operation_rollback_rewinds_gate_evaluating_state() {
    let root = unique_workspace("knots-write-dispatch-gate-rollback");
    setup_repo(&root);
    let app = open_app(&root);

    let gate = app
        .create_knot_with_options(
            "Release gate",
            None,
            None,
            None,
            None,
            CreateKnotOptions {
                knot_type: KnotType::Gate,
                gate_data: GateData {
                    owner_kind: GateOwnerKind::Agent,
                    ..Default::default()
                },
                ..CreateKnotOptions::default()
            },
        )
        .expect("gate should be created");
    let gate = app
        .set_state(
            &gate.id,
            crate::workflow_runtime::EVALUATING,
            false,
            gate.profile_etag.as_deref(),
        )
        .expect("gate should enter evaluating");

    let output = execute_operation(
        &app,
        &WriteOperation::Rollback(RollbackOperation {
            id: gate.id.clone(),
            dry_run: false,
            actor_kind: Some("agent".to_string()),
            agent_name: Some("rollbacker".to_string()),
            agent_model: Some("model".to_string()),
            agent_version: Some("1.0".to_string()),
        }),
    )
    .expect("gate rollback should succeed");
    assert!(output.contains("rolled back"));
    assert!(output.contains("ready_to_evaluate"));
    assert!(output.contains("owner: agent"));

    let reloaded = app
        .show_knot(&gate.id)
        .expect("gate should load")
        .expect("gate should exist");
    assert_eq!(reloaded.state, crate::workflow_runtime::READY_TO_EVALUATE);

    let _ = std::fs::remove_dir_all(root);
}
