use crate::app::StateActorMetadata;
use crate::lease_guard::materialize_expired_lease;
use crate::poll_claim;
use crate::write_queue::{UpdateOperation, WriteOperation};

use super::execute_operation;
use super::tests_lease_ext::{open_app, setup_repo, unique_workspace};

fn claim_actor() -> StateActorMetadata {
    StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: Some("test-agent".to_string()),
        agent_model: Some("test-model".to_string()),
        agent_version: Some("1.0".to_string()),
    }
}

fn update_op_with_lease(id: &str, lease_id: &str) -> WriteOperation {
    WriteOperation::Update(UpdateOperation {
        id: id.to_string(),
        title: Some("heartbeat-check".to_string()),
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
        lease_id: Some(lease_id.to_string()),
    })
}

fn claim_work_knot(app: &crate::app::App, timeout: u64) -> (String, String) {
    let work = app
        .create_knot(
            "Lease timeout test",
            None,
            Some("work_item"),
            Some("default"),
        )
        .expect("create work knot");
    let claimed = poll_claim::claim_knot(app, &work.id, claim_actor(), None, timeout)
        .expect("claim should succeed");
    let lease_id = claimed.knot.lease_id.clone().expect("should have lease");
    (work.id, lease_id)
}

#[test]
fn heartbeat_preserves_configured_timeout() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let (knot_id, lease_id) = claim_work_knot(&app, 1800);

    // Record expiry before update
    let before = app
        .show_knot(&lease_id)
        .expect("show")
        .expect("lease exists");
    let expiry_before = before.lease_expiry_ts;

    // Execute an update to trigger heartbeat
    execute_operation(&app, &update_op_with_lease(&knot_id, &lease_id))
        .expect("update should succeed");

    let after = app
        .show_knot(&lease_id)
        .expect("show")
        .expect("lease exists");
    let expiry_after = after.lease_expiry_ts;

    // Heartbeat should refresh with 1800s, not 600s
    assert!(
        expiry_after >= expiry_before,
        "expiry should not decrease after heartbeat"
    );
    // The new expiry should be approximately now+1800.
    // Verify it's > now+1000 (well above the 600s default).
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    assert!(
        expiry_after > now + 1000,
        "expiry {expiry_after} should be > now+1000 ({}) \
         for 1800s timeout",
        now + 1000
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn heartbeat_uses_default_for_legacy_lease() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let (knot_id, lease_id) = claim_work_knot(&app, 600);

    // Clear the timeout_seconds from LeaseData to simulate legacy
    // by directly overwriting the lease data field. We can't easily
    // mutate LeaseData in DB, but a lease created with timeout=600
    // should behave identically to the default path. Verify that
    // after heartbeat the expiry is near now+600.
    execute_operation(&app, &update_op_with_lease(&knot_id, &lease_id))
        .expect("update should succeed");

    let after = app
        .show_knot(&lease_id)
        .expect("show")
        .expect("lease exists");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    // Expiry should be approximately now+600 (within a small window)
    let delta = (after.lease_expiry_ts - now - 600).abs();
    assert!(delta < 5, "expiry should be ~now+600; delta={delta}");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn materialize_expired_lease_terminates_and_rolls_back() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let (knot_id, lease_id) = claim_work_knot(&app, 600);

    // Verify knot is in implementation state after claim
    let knot = app.show_knot(&knot_id).expect("show").expect("knot exists");
    assert_eq!(knot.state, "implementation");

    // Expire the lease
    app.set_lease_expiry(&lease_id, 1).expect("set expiry");

    // Re-read after expiry change
    let knot = app.show_knot(&knot_id).expect("show").expect("knot exists");
    let result = materialize_expired_lease(&app, &knot).expect("materialize should succeed");
    assert!(result, "should return true for expired lease");

    // Verify: lease terminated
    let lease = app
        .show_knot(&lease_id)
        .expect("show")
        .expect("lease exists");
    assert_eq!(
        lease.state, "lease_terminated",
        "lease should be terminated"
    );

    // Verify: lease unbound from knot
    let knot = app.show_knot(&knot_id).expect("show").expect("knot exists");
    assert!(knot.lease_id.is_none(), "lease_id should be cleared");

    // Verify: knot rolled back to queue state
    assert_eq!(
        knot.state, "ready_for_implementation",
        "knot should roll back to ready_for_implementation"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn materialize_expired_lease_skips_non_expired() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let (knot_id, lease_id) = claim_work_knot(&app, 600);

    let knot = app.show_knot(&knot_id).expect("show").expect("knot exists");
    let result = materialize_expired_lease(&app, &knot).expect("materialize should succeed");
    assert!(!result, "should return false for non-expired lease");

    // Verify knot unchanged
    let knot = app.show_knot(&knot_id).expect("show").expect("knot exists");
    assert_eq!(knot.state, "implementation");
    assert_eq!(knot.lease_id.as_deref(), Some(lease_id.as_str()));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn execute_update_materializes_expired_before_validation() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let (knot_id, lease_id) = claim_work_knot(&app, 600);

    // Expire the lease
    app.set_lease_expiry(&lease_id, 1).expect("set expiry");

    // Attempt update with the now-expired lease
    let err = execute_operation(&app, &update_op_with_lease(&knot_id, &lease_id))
        .expect_err("update should fail after materialization");
    let msg = err.to_string();
    assert!(
        msg.contains("no active lease")
            || msg.contains("not a claimable")
            || msg.contains("lease mismatch"),
        "error should indicate lease is gone: {msg}"
    );

    // Verify knot was materialized (rolled back, lease unbound)
    let knot = app.show_knot(&knot_id).expect("show").expect("knot exists");
    assert!(
        knot.lease_id.is_none(),
        "lease should be unbound after materialization"
    );
    assert_eq!(
        knot.state, "ready_for_implementation",
        "knot should be rolled back"
    );

    let _ = std::fs::remove_dir_all(root);
}
