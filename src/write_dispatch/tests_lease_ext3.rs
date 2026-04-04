use crate::app::StateActorMetadata;
use crate::poll_claim;
use crate::write_queue::{UpdateOperation, WriteOperation};

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
    let claimed =
        poll_claim::claim_knot(&app, &knot.id, claim_actor(true), None, 600).expect("claim");
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
    let claimed =
        poll_claim::claim_knot(&app, &knot.id, claim_actor(true), None, 600).expect("claim");
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
    let claimed =
        poll_claim::claim_knot(&app, &knot.id, claim_actor(false), None, 600).expect("claim");
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
