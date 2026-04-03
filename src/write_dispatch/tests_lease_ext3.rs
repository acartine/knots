use crate::app::StateActorMetadata;
use crate::poll_claim;
use crate::write_queue::{NextOperation, UpdateOperation, WriteOperation};

use super::execute_operation;
use super::tests_lease_ext::{create_test_lease, open_app, setup_repo, unique_workspace};

fn update_operation(id: &str, title: &str, lease_id: Option<String>) -> WriteOperation {
    WriteOperation::Update(UpdateOperation {
        id: id.to_string(),
        title: Some(title.to_string()),
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
        lease_id,
    })
}

fn claim_actor(with_agent_name: bool) -> StateActorMetadata {
    StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: with_agent_name.then(|| "test-agent".to_string()),
        agent_model: with_agent_name.then(|| "test-model".to_string()),
        agent_version: with_agent_name.then(|| "1.0".to_string()),
    }
}

#[test]
fn update_with_matching_lease_succeeds_without_rebinding() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let knot = app
        .create_knot(
            "Matching lease update",
            None,
            Some("work_item"),
            Some("default"),
        )
        .expect("create knot");
    let claimed = poll_claim::claim_knot(&app, &knot.id, claim_actor(true), None).expect("claim");
    let lease_id = claimed
        .knot
        .lease_id
        .clone()
        .expect("lease should be bound");

    execute_operation(
        &app,
        &update_operation(
            &knot.id,
            "Updated with matching lease",
            Some(lease_id.clone()),
        ),
    )
    .expect("matching lease should succeed");

    let updated = app.show_knot(&knot.id).expect("show").expect("knot exists");
    assert_eq!(updated.title, "Updated with matching lease");
    assert_eq!(updated.lease_id.as_deref(), Some(lease_id.as_str()));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn update_with_wrong_lease_fails_without_mutating() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let knot = app
        .create_knot(
            "Wrong lease update",
            None,
            Some("work_item"),
            Some("default"),
        )
        .expect("create knot");
    let claimed = poll_claim::claim_knot(&app, &knot.id, claim_actor(true), None).expect("claim");
    let lease_id = claimed
        .knot
        .lease_id
        .clone()
        .expect("lease should be bound");

    let err = execute_operation(
        &app,
        &update_operation(
            &knot.id,
            "Updated with wrong lease",
            Some("wrong-lease-id".to_string()),
        ),
    )
    .expect_err("wrong lease should fail");
    let err_msg = err.to_string();
    assert!(
        err_msg.contains("lease mismatch"),
        "error should mention lease mismatch: {err_msg}"
    );

    let updated = app.show_knot(&knot.id).expect("show").expect("knot exists");
    assert_eq!(updated.title, "Wrong lease update");
    assert_eq!(updated.lease_id.as_deref(), Some(lease_id.as_str()));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn claimed_without_lease_then_update_cannot_bind() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let knot = app
        .create_knot(
            "Unleased claim update",
            None,
            Some("work_item"),
            Some("default"),
        )
        .expect("create knot");
    let claimed = poll_claim::claim_knot(&app, &knot.id, claim_actor(false), None).expect("claim");
    assert!(
        claimed.knot.lease_id.is_none(),
        "claim should not create a lease"
    );

    let lease_id = create_test_lease(&app);
    let err = execute_operation(
        &app,
        &update_operation(&knot.id, "Updated after unleased claim", Some(lease_id)),
    )
    .expect_err("unleased knot should reject update lease");
    let err_msg = err.to_string();
    assert!(
        err_msg.contains("claim operations"),
        "error should mention claim-only binding: {err_msg}"
    );

    let updated = app.show_knot(&knot.id).expect("show").expect("knot exists");
    assert_eq!(updated.title, "Unleased claim update");
    assert!(updated.lease_id.is_none(), "update should not bind a lease");

    let _ = std::fs::remove_dir_all(root);
}

/// AC-4: next on a bounded knot clears lease_id and terminates the lease.
#[test]
fn next_clears_lease_and_terminates() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let work = app
        .create_knot("Clear lease test", None, Some("work_item"), Some("default"))
        .expect("create knot");

    let claimed = poll_claim::claim_knot(&app, &work.id, claim_actor(true), None).expect("claim");
    let lease_id = claimed.knot.lease_id.clone().expect("lease bound");

    let next_op = WriteOperation::Next(NextOperation {
        id: work.id.clone(),
        expected_state: Some(claimed.knot.state.clone()),
        json: false,
        approve_terminal_cascade: false,
        actor_kind: None,
        agent_name: None,
        agent_model: None,
        agent_version: None,
        lease_id: Some(lease_id.clone()),
    });
    execute_operation(&app, &next_op).expect("next should succeed");

    let knot_after = app.show_knot(&work.id).expect("show").expect("knot exists");
    assert!(
        knot_after.lease_id.is_none(),
        "lease_id should be cleared after next"
    );

    let lease_after = app
        .show_knot(&lease_id)
        .expect("show lease")
        .expect("lease exists");
    assert_eq!(
        lease_after.state, "lease_terminated",
        "lease should be terminated after next"
    );

    let _ = std::fs::remove_dir_all(root);
}

/// AC-3: next on a bounded knot rejects when lease_id is omitted.
#[test]
fn next_rejects_missing_lease_on_bounded_knot() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let work = app
        .create_knot(
            "Missing lease next",
            None,
            Some("work_item"),
            Some("default"),
        )
        .expect("create knot");

    let claimed = poll_claim::claim_knot(&app, &work.id, claim_actor(true), None).expect("claim");
    assert!(claimed.knot.lease_id.is_some(), "should have lease");

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
    let err = execute_operation(&app, &next_op).expect_err("should reject missing lease");
    let msg = err.to_string();
    assert!(
        msg.contains("must provide --lease"),
        "error should mention --lease: {msg}"
    );

    let _ = std::fs::remove_dir_all(root);
}

/// AC-3: next on a bounded knot rejects non-matching lease_id.
#[test]
fn next_rejects_wrong_lease_on_bounded_knot() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let work = app
        .create_knot("Wrong lease next", None, Some("work_item"), Some("default"))
        .expect("create knot");

    let claimed = poll_claim::claim_knot(&app, &work.id, claim_actor(true), None).expect("claim");

    let next_op = WriteOperation::Next(NextOperation {
        id: work.id.clone(),
        expected_state: Some(claimed.knot.state.clone()),
        json: false,
        approve_terminal_cascade: false,
        actor_kind: None,
        agent_name: None,
        agent_model: None,
        agent_version: None,
        lease_id: Some("wrong-id".to_string()),
    });
    let err = execute_operation(&app, &next_op).expect_err("should reject wrong lease");
    let msg = err.to_string();
    assert!(
        msg.contains("lease mismatch"),
        "error should mention mismatch: {msg}"
    );

    let _ = std::fs::remove_dir_all(root);
}

/// AC-3 variant: next with lease on an unleased knot fails.
#[test]
fn next_with_lease_on_unleased_knot_fails() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let work = app
        .create_knot(
            "Unleased knot next",
            None,
            Some("work_item"),
            Some("default"),
        )
        .expect("create knot");

    // Claim without agent_name so no lease is created
    let claimed = poll_claim::claim_knot(&app, &work.id, claim_actor(false), None).expect("claim");
    assert!(claimed.knot.lease_id.is_none(), "should not have lease");

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
    let err = execute_operation(&app, &next_op).expect_err("should fail when knot has no lease");
    let msg = err.to_string();
    assert!(
        msg.contains("no active lease"),
        "error should mention no active lease: {msg}"
    );

    let _ = std::fs::remove_dir_all(root);
}
