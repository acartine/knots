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
        json: false,
    })
}

fn claim_actor_kind() -> Option<String> {
    Some("agent".to_string())
}

#[test]
fn update_with_matching_lease_succeeds_without_rebinding() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
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
    let claimed = poll_claim::claim_knot(&app, &knot.id, claim_actor_kind(), None, 600, false)
        .expect("claim");
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
}

#[test]
fn update_with_wrong_lease_fails_without_mutating() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
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
    let claimed = poll_claim::claim_knot(&app, &knot.id, claim_actor_kind(), None, 600, false)
        .expect("claim");
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
}

#[test]
fn unleased_knot_rejects_update_with_lease_flag() {
    // Claim always auto-creates a lease now, so to exercise the "no bound
    // lease" path we claim, terminate the auto-created lease, and then try
    // to attach a different lease via `kno update --lease` (which is not
    // a claim operation and must be rejected).
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
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
    let claimed = poll_claim::claim_knot(&app, &knot.id, claim_actor_kind(), None, 600, false)
        .expect("claim");
    let auto_lease = claimed
        .knot
        .lease_id
        .clone()
        .expect("auto-created lease should exist");
    crate::lease::terminate_lease(&app, &auto_lease).expect("terminate");
    app.set_lease_id(&knot.id, None)
        .expect("unbind lease from knot");
    let after = app.show_knot(&knot.id).expect("show").expect("knot");
    assert!(
        after.lease_id.is_none(),
        "knot should have no bound lease after unbind"
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
}
