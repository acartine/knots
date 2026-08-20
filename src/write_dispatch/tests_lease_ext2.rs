use super::execute_operation;
use super::tests_lease_ext::{create_test_lease, open_app, setup_repo, unique_workspace};
use crate::app::App;
use crate::poll_claim::{self, PollResult};
use crate::write_queue::{NextOperation, UpdateOperation, WriteOperation};
fn claim_default(app: &App, id: &str) -> PollResult {
    poll_claim::claim_knot(app, id, Some("agent".to_string()), None, 600, false).expect("claim")
}
#[test]
fn explicit_note_agent_flags_are_ignored_lease_wins() {
    // Lease is the declared source of note agent identity. Deprecated
    // --note-* agent flags on `kno update` are ignored; the lease wins.
    // The non-agent --note-username override is preserved.
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
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
        add_verification_steps: vec![],
        remove_verification_steps: vec![],
        clear_verification_steps: false,
        gate_owner_kind: None,
        gate_failure_modes: vec![],
        clear_gate_failure_modes: false,
        scope: crate::cli_scope::ScopeArgs::default(),
        execution_plan_file: None,
        objective: None,
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
        json: false,
    });
    execute_operation(&app, &op).expect("update should succeed");

    let updated = app.show_knot(&knot.id).expect("show").expect("knot exists");
    let note = updated.notes.last().expect("should have a note");
    // Username is not agent identity and still flows from the caller.
    assert_eq!(note.username, "custom-user");
    // Agent-identity fields come from the bound lease's agent_info, not the
    // deprecated per-note flags.
    assert_eq!(note.agentname, "claude");
    assert_eq!(note.model, "opus");
    assert_eq!(note.version, "4.6");
}
#[test]
fn note_defaults_preserved_without_lease() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
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
        add_verification_steps: vec![],
        remove_verification_steps: vec![],
        clear_verification_steps: false,
        gate_owner_kind: None,
        gate_failure_modes: vec![],
        clear_gate_failure_modes: false,
        scope: crate::cli_scope::ScopeArgs::default(),
        execution_plan_file: None,
        objective: None,
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
        json: false,
    });
    execute_operation(&app, &op).expect("update should succeed");

    let updated = app.show_knot(&knot.id).expect("show").expect("knot exists");
    let note = updated.notes.last().expect("should have a note");
    assert_eq!(note.username, "unknown");
    assert_eq!(note.agentname, "unknown");
}
#[test]
fn handoff_capsule_auto_fills_from_lease_agent_info() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
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
        add_verification_steps: vec![],
        remove_verification_steps: vec![],
        clear_verification_steps: false,
        gate_owner_kind: None,
        gate_failure_modes: vec![],
        clear_gate_failure_modes: false,
        scope: crate::cli_scope::ScopeArgs::default(),
        execution_plan_file: None,
        objective: None,
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
        json: false,
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
}

#[test]
fn explicit_handoff_agent_flags_are_ignored_lease_wins() {
    // Lease is the declared source of handoff agent identity. Deprecated
    // --handoff-agentname / --handoff-model / --handoff-version values on
    // `kno update` must be ignored; the lease's agent_info wins. The
    // non-agent --handoff-username override is preserved.
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
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
        add_verification_steps: vec![],
        remove_verification_steps: vec![],
        clear_verification_steps: false,
        gate_owner_kind: None,
        gate_failure_modes: vec![],
        clear_gate_failure_modes: false,
        scope: crate::cli_scope::ScopeArgs::default(),
        execution_plan_file: None,
        objective: None,
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
        json: false,
    });
    execute_operation(&app, &op).expect("update should succeed");

    let updated = app.show_knot(&knot.id).expect("show").expect("exists");
    let hc = updated
        .handoff_capsules
        .last()
        .expect("should have handoff");
    assert_eq!(hc.username, "custom-user");
    assert_eq!(hc.agentname, "claude");
    assert_eq!(hc.model, "opus");
    assert_eq!(hc.version, "4.6");
}

#[test]
fn next_with_matching_lease_succeeds() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
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

    let claimed = claim_default(&app, &work.id);
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
}

#[test]
fn next_with_wrong_lease_fails() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let app = open_app(&root);

    let work = app
        .create_knot("Wrong lease next", None, Some("work_item"), Some("default"))
        .expect("create knot");

    let claimed = claim_default(&app, &work.id);
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
}

#[test]
fn next_without_lease_fails_when_knot_has_bound_lease() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let app = open_app(&root);

    let work = app
        .create_knot("No lease next", None, Some("work_item"), Some("default"))
        .expect("create knot");

    let claimed = claim_default(&app, &work.id);

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
        result.is_err(),
        "next without lease should fail for leased knots"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("bound lease"),
        "error should require the bound lease: {err}"
    );
}

#[test]
fn next_with_lease_on_unleasedknot_fails() {
    // Claim now always auto-creates a lease, so to reach the "knot has no
    // active lease" branch we must release the lease before running `next`.
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let app = open_app(&root);

    let work = app
        .create_knot("No lease on knot", None, Some("work_item"), Some("default"))
        .expect("create knot");

    let claimed = claim_default(&app, &work.id);
    let lease_id = claimed
        .knot
        .lease_id
        .clone()
        .expect("auto-created lease should exist");
    crate::lease::terminate_lease(&app, &lease_id).expect("terminate lease");
    app.set_lease_id(&work.id, None)
        .expect("unbind lease from knot");
    let after = app
        .show_knot(&work.id)
        .expect("show")
        .expect("knot should exist");
    assert!(
        after.lease_id.is_none(),
        "knot should have no bound lease after unbind"
    );

    let next_op = WriteOperation::Next(NextOperation {
        id: work.id.clone(),
        expected_state: Some(after.state.clone()),
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
}
