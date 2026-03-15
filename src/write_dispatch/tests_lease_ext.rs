use std::path::{Path, PathBuf};
use std::process::Command;

use uuid::Uuid;

use clap::Parser;

use super::{execute_operation, operation_from_command};
use crate::app::{App, StateActorMetadata};
use crate::cli::Cli;
use crate::poll_claim;
use crate::write_queue::{
    LeaseCreateOperation, LeaseTerminateOperation, NextOperation, WriteOperation,
};

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-wd-lease-ext-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
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
fn next_terminates_lease() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    // Create a work knot
    let work = app
        .create_knot("Lease next test", None, Some("work_item"), Some("default"))
        .expect("work knot should be created");

    // Claim it (which creates a lease via Phase 9c)
    let actor = StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: Some("test-agent".to_string()),
        agent_model: Some("test-model".to_string()),
        agent_version: Some("1.0".to_string()),
    };
    let claimed = poll_claim::claim_knot(&app, &work.id, actor).expect("claim should succeed");

    // Verify lease was created and bound
    let knot_after_claim = app
        .show_knot(&work.id)
        .expect("show should succeed")
        .expect("knot should exist");
    assert!(
        knot_after_claim.lease_id.is_some(),
        "claimed knot should have a lease_id"
    );
    let lease_id = knot_after_claim.lease_id.clone().expect("lease_id set");

    // Advance with next (implementation -> ready_for_review)
    let next_op = WriteOperation::Next(NextOperation {
        id: work.id.clone(),
        expected_state: Some(claimed.knot.state.clone()),
        json: false,
        approve_terminal_cascade: false,
        actor_kind: Some("agent".to_string()),
        agent_name: Some("test-agent".to_string()),
        agent_model: Some("test-model".to_string()),
        agent_version: Some("1.0".to_string()),
    });
    execute_operation(&app, &next_op).expect("next should succeed");

    // Verify lease is terminated and unbound
    let knot_after_next = app
        .show_knot(&work.id)
        .expect("show should succeed")
        .expect("knot should exist");
    assert!(
        knot_after_next.lease_id.is_none(),
        "lease_id should be cleared after next"
    );

    let lease_after = app
        .show_knot(&lease_id)
        .expect("show lease should succeed")
        .expect("lease knot should exist");
    assert_eq!(
        lease_after.state, "lease_terminated",
        "lease should be terminated after next"
    );

    let _ = std::fs::remove_dir_all(root);
}

fn parse(args: &[&str]) -> Cli {
    Cli::parse_from(args)
}

#[test]
fn operation_from_lease_create() {
    let cli = parse(&[
        "kno",
        "lease",
        "create",
        "--nickname",
        "sess",
        "--agent-type",
        "cli",
    ]);
    let op = operation_from_command(&cli.command);
    match op {
        Some(WriteOperation::LeaseCreate(c)) => {
            assert_eq!(c.nickname, "sess");
            assert_eq!(c.lease_type, "agent");
            assert_eq!(c.agent_type.as_deref(), Some("cli"));
        }
        other => {
            panic!("expected LeaseCreate, got {:?}", other)
        }
    }
}

#[test]
fn operation_from_lease_terminate() {
    let cli = parse(&["kno", "lease", "terminate", "knot-xyz"]);
    let op = operation_from_command(&cli.command);
    match op {
        Some(WriteOperation::LeaseTerminate(t)) => {
            assert_eq!(t.id, "knot-xyz");
        }
        other => {
            panic!("expected LeaseTerminate, got {:?}", other)
        }
    }
}

#[test]
fn operation_from_lease_show_is_none() {
    let cli = parse(&["kno", "lease", "show", "knot-abc"]);
    assert!(
        operation_from_command(&cli.command).is_none(),
        "show is a read op"
    );
}

#[test]
fn operation_from_lease_list_is_none() {
    let cli = parse(&["kno", "lease", "list"]);
    assert!(
        operation_from_command(&cli.command).is_none(),
        "list is a read op"
    );
}

#[test]
fn execute_lease_create_and_terminate() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let create_op = WriteOperation::LeaseCreate(LeaseCreateOperation {
        nickname: "test-agent".to_string(),
        lease_type: "agent".to_string(),
        agent_type: Some("cli".to_string()),
        provider: Some("Anthropic".to_string()),
        agent_name: Some("claude".to_string()),
        model: Some("opus".to_string()),
        model_version: Some("4.6".to_string()),
    });
    let output = execute_operation(&app, &create_op).expect("create should succeed");
    assert!(
        output.contains("created lease"),
        "should mention created: {output}"
    );

    let leases = crate::lease::list_active_leases(&app).expect("list should succeed");
    assert_eq!(leases.len(), 1);
    let lease_id = &leases[0].id;

    let term_op = WriteOperation::LeaseTerminate(LeaseTerminateOperation {
        id: lease_id.clone(),
    });
    let output = execute_operation(&app, &term_op).expect("terminate should succeed");
    assert!(
        output.contains("terminated lease"),
        "should mention terminated: {output}"
    );

    let after = crate::lease::list_active_leases(&app).expect("list should succeed");
    assert!(after.is_empty(), "no active leases after");

    let _ = std::fs::remove_dir_all(root);
}
