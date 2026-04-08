use super::*;
use crate::app::{App, AppError};
use crate::write_queue::{
    NewOperation, NextOperation, PollClaimOperation, QueuedWriteRequest, WriteOperation,
};

use clap::Parser;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;
use uuid::Uuid;

fn unique_workspace(prefix: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("temp workspace should be creatable");
    root
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
fn execute_queued_request_returns_failure_when_app_open_fails() {
    let root = unique_workspace("knots-write-dispatch-open-fail");
    setup_repo(&root);
    let bad_db_dir = root.join("db-directory");
    std::fs::create_dir_all(&bad_db_dir).expect("bad db directory should be creatable");
    let request = QueuedWriteRequest {
        request_id: "req-open-fail".to_string(),
        repo_root: root.to_string_lossy().into_owned(),
        store_root: root.join(".knots").to_string_lossy().into_owned(),
        distribution: "git".to_string(),
        project_id: None,
        db_path: bad_db_dir.to_string_lossy().into_owned(),
        response_path: String::new(),
        operation: WriteOperation::New(NewOperation {
            title: "queued".to_string(),
            description: None,
            acceptance: None,
            state: None,
            profile: None,
            workflow: None,
            fast: false,
            exploration: false,
            knot_type: None,
            gate_owner_kind: None,
            gate_failure_modes: vec![],
            lease_id: None,
        }),
    };
    let response = execute_queued_request(&request);
    assert!(!response.success);
    assert!(response
        .error
        .expect("error should be present")
        .contains("database"));
}

#[test]
fn execute_operation_poll_claim_empty_and_json() {
    let root = unique_workspace("knots-write-dispatch-poll-claim");
    setup_repo(&root);
    let db_path = root.join(".knots/cache/state.sqlite");
    let app = App::open(db_path.to_str().expect("utf8"), root.clone()).expect("app should open");
    let empty = WriteOperation::PollClaim(PollClaimOperation {
        stage: None,
        owner: None,
        json: false,
        agent_name: None,
        agent_model: None,
        agent_version: None,
        timeout_seconds: None,
    });
    let err = execute_operation(&app, &empty).expect_err("empty poll should fail");
    match err {
        AppError::InvalidArgument(msg) => {
            assert!(msg.contains("no claimable knots found"))
        }
        other => panic!("unexpected poll error: {other}"),
    }
    app.create_knot("Claim me", None, None, None)
        .expect("knot should be created");
    let json = WriteOperation::PollClaim(PollClaimOperation {
        stage: None,
        owner: None,
        json: true,
        agent_name: Some("agent".to_string()),
        agent_model: Some("model".to_string()),
        agent_version: Some("1.0".to_string()),
        timeout_seconds: None,
    });
    let output = execute_operation(&app, &json).expect("poll claim json should succeed");
    let parsed: serde_json::Value = serde_json::from_str(&output).expect("json parse");
    assert!(parsed
        .get("id")
        .and_then(serde_json::Value::as_str)
        .is_some());
}

#[test]
fn execute_operation_next_rejects_mismatched_state() {
    let root = unique_workspace("knots-write-dispatch-next-mismatch");
    setup_repo(&root);
    let db_path = root.join(".knots/cache/state.sqlite");
    let app = App::open(db_path.to_str().expect("utf8"), root.clone()).expect("app should open");
    let created = app
        .create_knot("Mismatch", None, Some("ready_for_implementation"), None)
        .expect("knot created");
    let op = WriteOperation::Next(NextOperation {
        id: created.id.clone(),
        expected_state: Some("planning".to_string()),
        json: false,
        approve_terminal_cascade: false,
        actor_kind: None,
        agent_name: None,
        agent_model: None,
        agent_version: None,
        lease_id: None,
    });
    let err = execute_operation(&app, &op).expect_err("mismatch");
    match err {
        AppError::InvalidArgument(msg) => {
            assert!(msg.contains("expected state 'planning'"));
            assert!(msg.contains("ready_for_implementation"));
        }
        other => panic!("unexpected: {other}"),
    }
}

#[test]
fn cascade_prompt_returns_error_in_noninteractive() {
    let descendants = vec![crate::state_hierarchy::HierarchyKnot {
        id: "knots-child".to_string(),
        state: "planning".to_string(),
        deferred_from_state: None,
        blocked_from_state: None,
    }];
    let err = helpers::execute_with_terminal_cascade_prompt(false, |_| -> Result<(), AppError> {
        Err(AppError::TerminalCascadeApprovalRequired {
            knot_id: "knots-parent".to_string(),
            target_state: "abandoned".to_string(),
            descendants: descendants.clone(),
        })
    })
    .expect_err("non-interactive should return error");
    match err {
        AppError::TerminalCascadeApprovalRequired {
            knot_id,
            target_state,
            descendants,
        } => {
            assert_eq!(knot_id, "knots-parent");
            assert_eq!(target_state, "abandoned");
            assert_eq!(descendants.len(), 1);
        }
        other => panic!("unexpected: {other}"),
    }
}

#[test]
fn cascade_prompt_accepts_yes() {
    let desc = vec![crate::state_hierarchy::HierarchyKnot {
        id: "knots-child".to_string(),
        state: "deferred".to_string(),
        deferred_from_state: Some("implementation".to_string()),
        blocked_from_state: None,
    }];
    let mut output = Vec::new();
    let mut input = Cursor::new("yes\n");
    let ok =
        helpers::terminal_cascade_prompt(&mut output, &mut input, "knots-parent", "shipped", &desc)
            .expect("prompt succeed");
    assert!(ok);
    let rendered = String::from_utf8(output).expect("utf8 output");
    assert!(rendered.contains("knots-parent"));
    assert!(rendered.contains("knots-child [deferred from implementation]"));
    assert!(rendered.contains("continue? [y/N]:"));
}

#[test]
fn cascade_prompt_rejects_non_yes() {
    let desc = vec![crate::state_hierarchy::HierarchyKnot {
        id: "knots-child".to_string(),
        state: "planning".to_string(),
        deferred_from_state: None,
        blocked_from_state: None,
    }];
    let mut output = Vec::new();
    let mut input = Cursor::new("no\n");
    let ok = helpers::terminal_cascade_prompt(
        &mut output,
        &mut input,
        "knots-parent",
        "abandoned",
        &desc,
    )
    .expect("prompt succeed");
    assert!(!ok);
}

#[test]
fn cascade_input_normalizes_yes_values() {
    assert!(helpers::is_terminal_cascade_approval("y"));
    assert!(helpers::is_terminal_cascade_approval(" YES "));
    assert!(!helpers::is_terminal_cascade_approval("n"));
    assert!(!helpers::is_terminal_cascade_approval(""));
}

#[test]
fn operation_from_command_threads_cascade_flags() {
    let state = crate::cli::Cli::parse_from([
        "kno",
        "state",
        "knots-1",
        "abandoned",
        "--cascade-terminal-descendants",
    ]);
    let update = crate::cli::Cli::parse_from([
        "kno",
        "update",
        "knots-1",
        "--status",
        "abandoned",
        "--cascade-terminal-descendants",
    ]);
    let next =
        crate::cli::Cli::parse_from(["kno", "next", "knots-1", "--cascade-terminal-descendants"]);
    match operation_from_command(&state.command).unwrap() {
        WriteOperation::State(op) => {
            assert!(op.approve_terminal_cascade)
        }
        other => panic!("unexpected: {other:?}"),
    }
    match operation_from_command(&update.command).unwrap() {
        WriteOperation::Update(op) => {
            assert!(op.approve_terminal_cascade)
        }
        other => panic!("unexpected: {other:?}"),
    }
    match operation_from_command(&next.command).unwrap() {
        WriteOperation::Next(op) => {
            assert!(op.approve_terminal_cascade)
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn operation_from_command_maps_rollback() {
    let cli =
        crate::cli::Cli::parse_from(["kno", "rb", "knots-1", "--dry-run", "--actor-kind", "agent"]);
    match operation_from_command(&cli.command).unwrap() {
        WriteOperation::Rollback(op) => {
            assert_eq!(op.id, "knots-1");
            assert!(op.dry_run);
            assert_eq!(op.actor_kind.as_deref(), Some("agent"));
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn operation_from_command_maps_step_annotate() {
    let cli = crate::cli::Cli::parse_from([
        "kno",
        "step",
        "annotate",
        "knots-1",
        "--actor-kind",
        "agent",
        "--agent-name",
        "codex",
        "--json",
    ]);
    match operation_from_command(&cli.command).unwrap() {
        WriteOperation::StepAnnotate(op) => {
            assert_eq!(op.id, "knots-1");
            assert_eq!(op.actor_kind.as_deref(), Some("agent"));
            assert_eq!(op.agent_name.as_deref(), Some("codex"));
            assert!(op.json);
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn maybe_run_queued_command_returns_none_for_read_only() {
    let cli = crate::cli::Cli::parse_from(["kno", "show", "knots-1"]);
    let result = maybe_run_queued_command(&cli).expect("read-only commands should skip queue");
    assert!(result.is_none());
}

#[test]
fn operation_from_command_maps_lease_extend() {
    let cli = crate::cli::Cli::parse_from([
        "kno",
        "lease",
        "extend",
        "--lease-id",
        "L-123",
        "--timeout-seconds",
        "900",
        "--json",
    ]);
    let op =
        operation_from_command(&cli.command).expect("lease extend should produce an operation");
    match op {
        WriteOperation::LeaseExtend(ext) => {
            assert_eq!(ext.lease_id, "L-123");
            assert_eq!(ext.timeout_seconds, Some(900));
            assert!(ext.json);
        }
        other => panic!("unexpected: {other:?}"),
    }
}

fn exploration_op() -> NewOperation {
    NewOperation {
        title: "Explore caching".to_string(),
        description: None,
        acceptance: None,
        state: None,
        profile: None,
        workflow: None,
        fast: false,
        exploration: true,
        knot_type: None,
        gate_owner_kind: None,
        gate_failure_modes: vec![],
        lease_id: None,
    }
}

#[test]
fn exploration_rejects_combined_fast_flag() {
    let root = unique_workspace("knots-wd-explore-fast");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");
    let app = App::open(db.to_str().unwrap(), root.clone()).unwrap();
    let mut op = exploration_op();
    op.fast = true;
    let err = execute_operation(&app, &WriteOperation::New(op))
        .expect_err("fast+exploration should fail");
    assert!(err.to_string().contains("cannot combine"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn exploration_rejects_combined_profile_flag() {
    let root = unique_workspace("knots-wd-explore-profile");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");
    let app = App::open(db.to_str().unwrap(), root.clone()).unwrap();
    let mut op = exploration_op();
    op.profile = Some("autopilot".to_string());
    let err = execute_operation(&app, &WriteOperation::New(op))
        .expect_err("profile+exploration should fail");
    assert!(err.to_string().contains("cannot combine"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn exploration_rejects_combined_workflow_flag() {
    let root = unique_workspace("knots-wd-explore-workflow");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");
    let app = App::open(db.to_str().unwrap(), root.clone()).unwrap();
    let mut op = exploration_op();
    op.workflow = Some("custom".to_string());
    let err = execute_operation(&app, &WriteOperation::New(op))
        .expect_err("workflow+exploration should fail");
    assert!(err.to_string().contains("cannot combine"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn exploration_new_creates_knot_with_explore_type() {
    let root = unique_workspace("knots-wd-explore-new");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");
    let app = App::open(db.to_str().unwrap(), root.clone()).unwrap();
    let output = execute_operation(&app, &WriteOperation::New(exploration_op()))
        .expect("exploration new should succeed");
    let lower = output.to_ascii_lowercase();
    assert!(lower.contains("ready_for_exploration"), "output: {output}");
    let knots = app.list_knots().expect("list should succeed");
    assert_eq!(knots.len(), 1);
    assert_eq!(
        knots[0].knot_type,
        crate::domain::knot_type::KnotType::Explore
    );
    let _ = std::fs::remove_dir_all(&root);
}
