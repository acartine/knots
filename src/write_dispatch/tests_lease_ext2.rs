use super::{execute_operation, operation_from_command};
use crate::app::StateActorMetadata;
use crate::poll_claim;
use crate::write_queue::{NextOperation, UpdateOperation, WriteOperation};

use super::tests_lease_ext::{create_test_lease, open_app, parse, setup_repo, unique_workspace};

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
        note_username: Some("custom-user".to_string()),
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
    assert_eq!(note.username, "custom-user");
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
    assert_eq!(note.username, "unknown");
    assert_eq!(note.agentname, "unknown");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn handoff_capsule_auto_fills_from_lease_agent_info() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let knot = app
        .create_knot("Handoff autofill test", None, None, None)
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
        add_note: None,
        note_username: None,
        note_datetime: None,
        note_agentname: None,
        note_model: None,
        note_version: None,
        add_handoff_capsule: Some("auto-filled handoff".to_string()),
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
    execute_operation(&app, &op).expect("update with handoff should succeed");

    let updated = app.show_knot(&knot.id).expect("show").expect("exists");
    let hc = updated
        .handoff_capsules
        .last()
        .expect("should have handoff");
    assert_eq!(hc.username, "Anthropic");
    assert_eq!(hc.agentname, "claude");
    assert_eq!(hc.model, "opus");
    assert_eq!(hc.version, "4.6");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn explicit_handoff_flags_override_lease_agent_info() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let knot = app
        .create_knot("Handoff override test", None, None, None)
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
        add_note: None,
        note_username: None,
        note_datetime: None,
        note_agentname: None,
        note_model: None,
        note_version: None,
        add_handoff_capsule: Some("override handoff".to_string()),
        handoff_username: Some("custom-user".to_string()),
        handoff_datetime: None,
        handoff_agentname: Some("custom-agent".to_string()),
        handoff_model: Some("custom-model".to_string()),
        handoff_version: Some("9.9".to_string()),
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

    let updated = app.show_knot(&knot.id).expect("show").expect("exists");
    let hc = updated
        .handoff_capsules
        .last()
        .expect("should have handoff");
    assert_eq!(hc.username, "custom-user");
    assert_eq!(hc.agentname, "custom-agent");
    assert_eq!(hc.model, "custom-model");
    assert_eq!(hc.version, "9.9");

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
fn next_without_lease_on_unleased_knot_works() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let work = app
        .create_knot("No lease next", None, Some("work_item"), Some("default"))
        .expect("create knot");

    // Claim without agent_name so no lease is created
    let actor = StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: None,
        agent_model: None,
        agent_version: None,
    };
    let claimed = poll_claim::claim_knot(&app, &work.id, actor, None).expect("claim");
    assert!(claimed.knot.lease_id.is_none(), "should have no lease");

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
        "next without lease on unleased knot should work"
    );

    let _ = std::fs::remove_dir_all(root);
}
