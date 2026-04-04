use crate::app::StateActorMetadata;

use super::claim_knot;
use super::tests_lease_ext::{create_agent_info, open_app, setup_repo, unique_workspace};

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

    let actor = StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: Some("test-agent".to_string()),
        ..Default::default()
    };
    let err = match claim_knot(&app, &work.id, actor, Some(&lease.id), 600) {
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
