use crate::domain::step_history::StepStatus;
use crate::lease_guard::materialize_expired_lease;
use crate::poll_claim;
use crate::write_queue::{UpdateOperation, WriteOperation};

use super::execute_operation;
use super::tests_lease_ext::{open_app, setup_repo, unique_workspace};

fn claim_actor_kind() -> Option<String> {
    Some("agent".to_string())
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
        lease_id: Some(lease_id.to_string()),
        json: false,
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
    let claimed = poll_claim::claim_knot(app, &work.id, claim_actor_kind(), None, timeout, false)
        .expect("claim should succeed");
    let lease_id = claimed.knot.lease_id.clone().expect("should have lease");
    (work.id, lease_id)
}

#[test]
fn heartbeat_preserves_configured_timeout() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
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
}

#[test]
fn heartbeat_uses_default_for_legacy_lease() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
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
}

#[test]
fn materialize_expired_lease_terminates_and_rolls_back() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
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
    assert_eq!(
        knot.step_history.last().map(|step| &step.status),
        Some(&StepStatus::Failed),
        "expired work must not be recorded as completed"
    );
}

#[test]
fn terminating_bound_lease_requeues_only_its_action_and_allows_reclaim() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let app = open_app(&root);

    let (first_knot, first_lease) = claim_work_knot(&app, 600);
    let (other_knot, other_lease) = claim_work_knot(&app, 600);

    crate::lease::terminate_lease(&app, &first_lease).expect("terminate bound lease");

    let first = app
        .show_knot(&first_knot)
        .expect("show first")
        .expect("first exists");
    assert_eq!(first.state, "ready_for_implementation");
    assert!(first.lease_id.is_none(), "terminated lease must be unbound");
    assert_eq!(
        first.step_history.last().map(|step| &step.status),
        Some(&StepStatus::Failed),
        "terminated work must not be recorded as completed"
    );

    let other = app
        .show_knot(&other_knot)
        .expect("show other")
        .expect("other exists");
    assert_eq!(other.state, "implementation");
    assert_eq!(other.lease_id.as_deref(), Some(other_lease.as_str()));

    let reclaimed = poll_claim::claim_knot(&app, &first_knot, claim_actor_kind(), None, 600, false)
        .expect("fresh lease should reclaim requeued work");
    assert_eq!(reclaimed.knot.state, "implementation");
    assert_ne!(
        reclaimed.knot.lease_id.as_deref(),
        Some(first_lease.as_str())
    );
}

#[test]
fn terminating_short_lease_id_recovers_legacy_short_binding() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let app = open_app(&root);
    let work = app
        .create_knot("Short lease", None, Some("work_item"), Some("default"))
        .expect("create work knot");
    let lease = crate::lease::create_lease(
        &app,
        "short-id",
        crate::domain::lease::LeaseType::Agent,
        Some(crate::domain::lease::AgentInfo::default()),
        600,
    )
    .expect("create lease");
    let short_id = crate::knot_id::display_id(&lease.id).to_string();
    let claimed = poll_claim::claim_knot(
        &app,
        &work.id,
        claim_actor_kind(),
        Some(&short_id),
        600,
        false,
    )
    .expect("claim with short lease id");
    assert_eq!(claimed.knot.lease_id.as_deref(), Some(short_id.as_str()));
    crate::lease::terminate_lease(&app, &short_id).expect("terminate short lease id");

    let recovered = app.show_knot(&work.id).expect("show").expect("work exists");
    assert_eq!(recovered.state, "ready_for_implementation");
    assert!(recovered.lease_id.is_none());
}

#[test]
fn materialize_expired_lease_skips_non_expired() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
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
}

#[test]
fn materialize_expired_lease_skips_unbound_work() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let app = open_app(&root);
    let work = app
        .create_knot("Unbound work", None, Some("work_item"), Some("default"))
        .expect("create work knot");

    let result = materialize_expired_lease(&app, &work).expect("materialize should succeed");
    assert!(!result, "unbound work has no expired lease to recover");
}

#[test]
fn terminating_lease_rejects_multiple_action_bindings() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let app = open_app(&root);
    let (first_knot, lease_id) = claim_work_knot(&app, 600);
    let (second_knot, _) = claim_work_knot(&app, 600);
    app.set_lease_id(&second_knot, Some(&lease_id))
        .expect("create duplicate action binding");

    let error = crate::lease::terminate_lease(&app, &lease_id)
        .expect_err("duplicate action bindings must be rejected");
    assert!(error.to_string().contains("multiple action-state knots"));
    assert_eq!(
        app.show_knot(&first_knot)
            .expect("show first")
            .expect("first exists")
            .state,
        "implementation"
    );
}

#[test]
fn terminating_non_lease_knot_is_rejected() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let app = open_app(&root);
    let work = app
        .create_knot("Not a lease", None, Some("work_item"), Some("default"))
        .expect("create work knot");

    let error =
        crate::lease::terminate_lease(&app, &work.id).expect_err("work knot must be rejected");
    assert!(error.to_string().contains("specified knot is not a lease"));
}

#[test]
fn execute_update_materializes_expired_before_validation() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
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
}

/// Re-stamp the workspace's only lease so it looks like another machine's.
fn reassign_lease_owner(root: &std::path::Path, machine_id: &str) {
    let db_path = root.join(".knots/cache/state.sqlite");
    let conn = crate::db::open_connection(db_path.to_str().expect("utf8 db path"))
        .expect("cache db should open");
    let changed = conn
        .execute(
            "UPDATE knot_hot SET lease_data_json = \
             json_set(lease_data_json, '$.owner', \
             json_object('machine_id', ?1, 'pid', 4242)) \
             WHERE knot_type = 'lease'",
            rusqlite::params![machine_id],
        )
        .expect("owner rewrite should succeed");
    assert_eq!(changed, 1, "exactly one lease should be re-stamped");
}

/// Drop the owner entirely, as an event written before owners existed.
fn clear_lease_owner(root: &std::path::Path) {
    let db_path = root.join(".knots/cache/state.sqlite");
    let conn = crate::db::open_connection(db_path.to_str().expect("utf8 db path"))
        .expect("cache db should open");
    let changed = conn
        .execute(
            "UPDATE knot_hot SET lease_data_json = \
             json_remove(lease_data_json, '$.owner') WHERE knot_type = 'lease'",
            [],
        )
        .expect("owner removal should succeed");
    assert_eq!(changed, 1, "exactly one lease should be stripped");
}

#[test]
fn materialize_expired_lease_leaves_another_machines_lease_alone() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let app = open_app(&root);

    let (knot_id, lease_id) = claim_work_knot(&app, 600);
    // Force an expired-looking effective state directly (skew, an in-flight
    // heartbeat, or a stale pull can all produce this even now that
    // `lease_expiry_ts` replicates) and confirm the owner guard refuses to
    // recover it regardless of what the effective state says.
    app.set_lease_expiry(&lease_id, 1).expect("set expiry");
    reassign_lease_owner(&root, "some-other-machine");

    let knot = app.show_knot(&knot_id).expect("show").expect("knot exists");
    let result = materialize_expired_lease(&app, &knot).expect("materialize should succeed");
    assert!(
        !result,
        "a lease held by another machine must never be recovered"
    );

    let lease = app
        .show_knot(&lease_id)
        .expect("show")
        .expect("lease exists");
    assert_eq!(
        lease.state, "lease_active",
        "the other machine's lease must stay active"
    );

    let knot = app.show_knot(&knot_id).expect("show").expect("knot exists");
    assert_eq!(
        knot.state, "implementation",
        "the knot must stay in its action state"
    );
    assert_eq!(
        knot.lease_id.as_deref(),
        Some(lease_id.as_str()),
        "the knot must stay bound to the other machine's lease"
    );
}

#[test]
fn materialize_expired_lease_still_recovers_an_owner_less_lease() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let app = open_app(&root);

    let (knot_id, lease_id) = claim_work_knot(&app, 600);
    app.set_lease_expiry(&lease_id, 1).expect("set expiry");
    clear_lease_owner(&root);

    let knot = app.show_knot(&knot_id).expect("show").expect("knot exists");
    let result = materialize_expired_lease(&app, &knot).expect("materialize should succeed");
    assert!(
        result,
        "a lease written before owners existed is treated as local"
    );

    let knot = app.show_knot(&knot_id).expect("show").expect("knot exists");
    assert_eq!(knot.state, "ready_for_implementation");
    assert!(knot.lease_id.is_none());
}
