use super::tests_lease_ext::{create_agent_info, open_app, setup_repo, unique_workspace};
use super::{claim_knot, compensate_failed_claim};
use crate::domain::step_history::StepStatus;

#[test]
fn failed_claim_compensation_requeues_and_fails_open_step() {
    let root = unique_workspace();
    let app = open_app(&root);
    let work = app
        .create_knot(
            "Claim compensation",
            None,
            Some("work_item"),
            Some("default"),
        )
        .expect("work knot should be created");
    app.set_state(
        &work.id,
        "implementation",
        false,
        work.profile_etag.as_deref(),
    )
    .expect("action state should be entered");

    compensate_failed_claim(&app, &work.id);

    let recovered = app
        .show_knot(&work.id)
        .expect("show should succeed")
        .expect("knot should exist");
    assert_eq!(recovered.state, "ready_for_implementation");
    assert!(recovered.lease_id.is_none());
    assert_eq!(
        recovered.step_history.last().map(|step| &step.status),
        Some(&StepStatus::Failed)
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn claim_with_active_external_lease_rejects() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let work = app
        .create_knot(
            "Active lease reject",
            None,
            Some("work_item"),
            Some("default"),
        )
        .expect("create work knot");
    let lease = crate::lease::create_lease(
        &app,
        "active-lease",
        crate::domain::lease::LeaseType::Agent,
        Some(create_agent_info()),
        600,
    )
    .expect("create lease");
    crate::lease::activate_lease(&app, &lease.id).expect("activate lease");

    let err = match claim_knot(
        &app,
        &work.id,
        Some("agent".to_string()),
        Some(&lease.id),
        600,
        false,
    ) {
        Err(err) => err.to_string(),
        Ok(_) => panic!("claim should reject active external lease"),
    };
    assert!(
        err.contains("expected 'lease_ready'"),
        "error should mention ready-only external leases: {err}"
    );

    let work_after = app.show_knot(&work.id).expect("show").expect("work exists");
    assert_eq!(work_after.state, "ready_for_implementation");
    assert!(
        work_after.lease_id.is_none(),
        "claim should not bind the lease"
    );

    let _ = std::fs::remove_dir_all(root);
}
