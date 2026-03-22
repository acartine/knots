use std::path::{Path, PathBuf};
use std::process::Command;

use uuid::Uuid;

use clap::Parser;

use super::{execute_operation, operation_from_command};
use crate::app::{App, StateActorMetadata};
use crate::cli::Cli;
use crate::poll_claim;
use crate::write_queue::{
    LeaseCreateOperation, LeaseTerminateOperation, NewOperation, NextOperation, UpdateOperation,
    WriteOperation,
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
    let claimed =
        poll_claim::claim_knot(&app, &work.id, actor, None).expect("claim should succeed");

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
        lease_id: None,
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
        json: false,
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

#[test]
fn lease_create_json_output() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let op = WriteOperation::LeaseCreate(LeaseCreateOperation {
        nickname: "json-test".to_string(),
        lease_type: "agent".to_string(),
        agent_type: Some("cli".to_string()),
        provider: Some("Anthropic".to_string()),
        agent_name: Some("claude".to_string()),
        model: Some("opus".to_string()),
        model_version: Some("4.6".to_string()),
        json: true,
    });
    let output = execute_operation(&app, &op).expect("create should succeed");
    let parsed: serde_json::Value =
        serde_json::from_str(&output).expect("output should be valid JSON");
    assert!(parsed["id"].is_string(), "JSON should contain id");
    assert_eq!(parsed["title"].as_str().unwrap(), "Lease: json-test");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn lease_create_text_output_when_json_false() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let op = WriteOperation::LeaseCreate(LeaseCreateOperation {
        nickname: "text-test".to_string(),
        lease_type: "agent".to_string(),
        agent_type: None,
        provider: None,
        agent_name: None,
        model: None,
        model_version: None,
        json: false,
    });
    let output = execute_operation(&app, &op).expect("create should succeed");
    assert!(
        output.contains("created lease"),
        "text output should contain 'created lease': {output}"
    );

    let _ = std::fs::remove_dir_all(root);
}

fn create_test_lease(app: &App) -> String {
    let op = WriteOperation::LeaseCreate(LeaseCreateOperation {
        nickname: "test-lease".to_string(),
        lease_type: "agent".to_string(),
        agent_type: Some("cli".to_string()),
        provider: Some("Anthropic".to_string()),
        agent_name: Some("claude".to_string()),
        model: Some("opus".to_string()),
        model_version: Some("4.6".to_string()),
        json: false,
    });
    execute_operation(app, &op).expect("lease create should succeed");
    let leases = crate::lease::list_active_leases(app).expect("list");
    leases.into_iter().last().expect("at least one lease").id
}

#[test]
fn new_with_lease_flag_binds_lease() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let lease_id = create_test_lease(&app);

    let op = WriteOperation::New(NewOperation {
        title: "Lease-bound new".to_string(),
        description: None,
        acceptance: None,
        state: None,
        profile: None,
        fast: false,
        knot_type: None,
        gate_owner_kind: None,
        gate_failure_modes: vec![],
        lease_id: Some(lease_id.clone()),
    });
    let output = execute_operation(&app, &op).expect("new should succeed");
    assert!(output.contains("created"), "should confirm creation");

    let knots = app.list_knots().expect("list");
    let work = knots
        .iter()
        .find(|k| k.title == "Lease-bound new")
        .expect("knot should exist");
    assert_eq!(
        work.lease_id.as_deref(),
        Some(lease_id.as_str()),
        "lease_id should be set"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn update_with_lease_flag_binds_lease() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let knot = app
        .create_knot("Update lease test", None, None, None)
        .expect("create");
    let lease_id = create_test_lease(&app);

    let op = WriteOperation::Update(UpdateOperation {
        id: knot.id.clone(),
        title: Some("Updated with lease".to_string()),
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
        gate_owner_kind: None,
        gate_failure_modes: vec![],
        clear_gate_failure_modes: false,
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
        lease_id: Some(lease_id.clone()),
    });
    execute_operation(&app, &op).expect("update should succeed");

    let updated = app.show_knot(&knot.id).expect("show").expect("knot exists");
    assert_eq!(updated.title, "Updated with lease");
    assert_eq!(
        updated.lease_id.as_deref(),
        Some(lease_id.as_str()),
        "lease_id should be bound via update"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn note_auto_fills_from_lease_agent_info() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let knot = app
        .create_knot("Note autofill test", None, None, None)
        .expect("create");
    let lease_id = create_test_lease(&app);
    crate::lease::bind_lease(&app, &knot.id, &lease_id).expect("bind");

    let op = WriteOperation::Update(UpdateOperation {
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
        gate_owner_kind: None,
        gate_failure_modes: vec![],
        clear_gate_failure_modes: false,
        add_note: Some("auto-filled note".to_string()),
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
    });
    execute_operation(&app, &op).expect("update with note should succeed");

    let updated = app.show_knot(&knot.id).expect("show").expect("knot exists");
    let note = updated.notes.last().expect("should have a note");
    assert_eq!(note.agentname, "claude");
    assert_eq!(note.model, "opus");
    assert_eq!(note.version, "4.6");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn explicit_note_flags_override_lease_agent_info() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let knot = app
        .create_knot("Override test", None, None, None)
        .expect("create");
    let lease_id = create_test_lease(&app);
    crate::lease::bind_lease(&app, &knot.id, &lease_id).expect("bind");

    let op = WriteOperation::Update(UpdateOperation {
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
        gate_owner_kind: None,
        gate_failure_modes: vec![],
        clear_gate_failure_modes: false,
        add_note: Some("override note".to_string()),
        note_username: None,
        note_datetime: None,
        note_agentname: Some("custom-agent".to_string()),
        note_model: Some("custom-model".to_string()),
        note_version: Some("9.9".to_string()),
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
    });
    execute_operation(&app, &op).expect("update should succeed");

    let updated = app.show_knot(&knot.id).expect("show").expect("knot exists");
    let note = updated.notes.last().expect("should have a note");
    assert_eq!(note.agentname, "custom-agent");
    assert_eq!(note.model, "custom-model");
    assert_eq!(note.version, "9.9");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn note_defaults_preserved_without_lease() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let knot = app
        .create_knot("No lease test", None, None, None)
        .expect("create");

    let op = WriteOperation::Update(UpdateOperation {
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
        gate_owner_kind: None,
        gate_failure_modes: vec![],
        clear_gate_failure_modes: false,
        add_note: Some("plain note".to_string()),
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
    });
    execute_operation(&app, &op).expect("update should succeed");

    let updated = app.show_knot(&knot.id).expect("show").expect("knot exists");
    let note = updated.notes.last().expect("should have a note");
    assert_eq!(note.agentname, "unknown");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn operation_from_lease_create_includes_json() {
    let cli = parse(&["kno", "lease", "create", "--nickname", "sess", "--json"]);
    let op = operation_from_command(&cli.command);
    match op {
        Some(WriteOperation::LeaseCreate(c)) => {
            assert!(c.json, "json flag should be true");
        }
        other => panic!("expected LeaseCreate, got {:?}", other),
    }
}

#[test]
fn operation_from_new_includes_lease_id() {
    let cli = parse(&["kno", "new", "My title", "--lease", "lease-abc"]);
    let op = operation_from_command(&cli.command);
    match op {
        Some(WriteOperation::New(n)) => {
            assert_eq!(n.lease_id.as_deref(), Some("lease-abc"));
        }
        other => panic!("expected New, got {:?}", other),
    }
}

#[test]
fn operation_from_update_includes_lease_id() {
    let cli = parse(&["kno", "update", "knot-xyz", "--lease", "lease-abc"]);
    let op = operation_from_command(&cli.command);
    match op {
        Some(WriteOperation::Update(u)) => {
            assert_eq!(u.lease_id.as_deref(), Some("lease-abc"));
        }
        other => panic!("expected Update, got {:?}", other),
    }
}

#[test]
fn operation_from_claim_includes_lease_id() {
    let cli = parse(&["kno", "claim", "knot-xyz", "--lease", "lease-abc"]);
    let op = operation_from_command(&cli.command);
    match op {
        Some(WriteOperation::Claim(c)) => {
            assert_eq!(c.lease_id.as_deref(), Some("lease-abc"));
        }
        other => panic!("expected Claim, got {:?}", other),
    }
}

#[test]
fn operation_from_next_includes_lease_id() {
    let cli = parse(&[
        "kno",
        "next",
        "knot-xyz",
        "--expected-state",
        "implementation",
        "--lease",
        "lease-abc",
    ]);
    let op = operation_from_command(&cli.command);
    match op {
        Some(WriteOperation::Next(n)) => {
            assert_eq!(n.lease_id.as_deref(), Some("lease-abc"));
        }
        other => panic!("expected Next, got {:?}", other),
    }
}

#[test]
fn next_with_matching_lease_succeeds() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let work = app
        .create_knot(
            "Matching lease next",
            None,
            Some("work_item"),
            Some("default"),
        )
        .expect("create knot");

    let actor = StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: Some("test-agent".to_string()),
        agent_model: Some("test-model".to_string()),
        agent_version: Some("1.0".to_string()),
    };
    let claimed = poll_claim::claim_knot(&app, &work.id, actor, None).expect("claim");
    let lease_id = claimed.knot.lease_id.clone().expect("should have lease");

    let next_op = WriteOperation::Next(NextOperation {
        id: work.id.clone(),
        expected_state: Some(claimed.knot.state.clone()),
        json: false,
        approve_terminal_cascade: false,
        actor_kind: None,
        agent_name: None,
        agent_model: None,
        agent_version: None,
        lease_id: Some(lease_id),
    });
    let result = execute_operation(&app, &next_op);
    assert!(result.is_ok(), "next with matching lease should succeed");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn next_with_wrong_lease_fails() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let work = app
        .create_knot("Wrong lease next", None, Some("work_item"), Some("default"))
        .expect("create knot");

    let actor = StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: Some("test-agent".to_string()),
        agent_model: Some("test-model".to_string()),
        agent_version: Some("1.0".to_string()),
    };
    let claimed = poll_claim::claim_knot(&app, &work.id, actor, None).expect("claim");

    let next_op = WriteOperation::Next(NextOperation {
        id: work.id.clone(),
        expected_state: Some(claimed.knot.state.clone()),
        json: false,
        approve_terminal_cascade: false,
        actor_kind: None,
        agent_name: None,
        agent_model: None,
        agent_version: None,
        lease_id: Some("wrong-lease-id".to_string()),
    });
    let result = execute_operation(&app, &next_op);
    assert!(result.is_err(), "next with wrong lease should fail");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("lease mismatch"),
        "error should mention lease mismatch: {}",
        err_msg
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn next_without_lease_still_works() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let work = app
        .create_knot("No lease next", None, Some("work_item"), Some("default"))
        .expect("create knot");

    let actor = StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: Some("test-agent".to_string()),
        agent_model: Some("test-model".to_string()),
        agent_version: Some("1.0".to_string()),
    };
    let claimed = poll_claim::claim_knot(&app, &work.id, actor, None).expect("claim");

    let next_op = WriteOperation::Next(NextOperation {
        id: work.id.clone(),
        expected_state: Some(claimed.knot.state.clone()),
        json: false,
        approve_terminal_cascade: false,
        actor_kind: None,
        agent_name: None,
        agent_model: None,
        agent_version: None,
        lease_id: None,
    });
    let result = execute_operation(&app, &next_op);
    assert!(
        result.is_ok(),
        "next without lease should still work (backwards compat)"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn next_with_lease_on_unleasedknot_fails() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let work = app
        .create_knot("No lease on knot", None, Some("work_item"), Some("default"))
        .expect("create knot");

    // Claim without creating a lease (no agent_name)
    let actor = StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: None,
        agent_model: None,
        agent_version: None,
    };
    let claimed = poll_claim::claim_knot(&app, &work.id, actor, None).expect("claim");
    assert!(claimed.knot.lease_id.is_none(), "should not have a lease");

    let next_op = WriteOperation::Next(NextOperation {
        id: work.id.clone(),
        expected_state: Some(claimed.knot.state.clone()),
        json: false,
        approve_terminal_cascade: false,
        actor_kind: None,
        agent_name: None,
        agent_model: None,
        agent_version: None,
        lease_id: Some("fake-lease".to_string()),
    });
    let result = execute_operation(&app, &next_op);
    assert!(result.is_err(), "should fail when knot has no lease");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("no active lease"),
        "error should mention no active lease: {err}"
    );

    let _ = std::fs::remove_dir_all(root);
}
