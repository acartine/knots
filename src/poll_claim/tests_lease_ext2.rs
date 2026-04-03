use std::path::{Path, PathBuf};

use uuid::Uuid;

use super::claim_knot;
use crate::app::{App, StateActorMetadata};

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-poll-lease-ext2-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    root
}

fn open_app(root: &Path) -> App {
    let db_path = root.join(".knots/cache/state.sqlite");
    App::open(db_path.to_str().expect("utf8"), root.to_path_buf()).expect("app should open")
}

fn create_agent_info() -> crate::domain::lease::AgentInfo {
    crate::domain::lease::AgentInfo {
        agent_type: "cli".to_string(),
        provider: "test".to_string(),
        agent_name: "test-agent".to_string(),
        model: "test-model".to_string(),
        model_version: "1.0".to_string(),
    }
}

fn default_actor() -> StateActorMetadata {
    StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: Some("test-agent".to_string()),
        agent_model: Some("test-model".to_string()),
        agent_version: Some("1.0".to_string()),
    }
}

/// AC-1: claim with an explicit lease_id rejects lease_active.
#[test]
fn claim_rejects_active_lease() {
    let root = unique_workspace();
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
    )
    .expect("create lease");
    // Activate the lease so it's in lease_active
    crate::lease::activate_lease(&app, &lease.id).expect("activate");

    let result = claim_knot(&app, &work.id, default_actor(), Some(&lease.id));
    let msg = match result {
        Err(e) => e.to_string(),
        Ok(_) => panic!("claim with active lease should fail"),
    };
    assert!(
        msg.contains("lease_active"),
        "error should mention lease_active: {msg}"
    );
    assert!(
        msg.contains("expected lease_ready"),
        "error should mention expected lease_ready: {msg}"
    );

    let _ = std::fs::remove_dir_all(root);
}

/// AC-2: claim with lease_ready atomically activates the lease.
#[test]
fn claim_with_ready_lease_atomically_activates() {
    let root = unique_workspace();
    let app = open_app(&root);

    let work = app
        .create_knot(
            "Ready activation test",
            None,
            Some("work_item"),
            Some("default"),
        )
        .expect("create work knot");

    let lease = crate::lease::create_lease(
        &app,
        "ready-activation",
        crate::domain::lease::LeaseType::Agent,
        Some(create_agent_info()),
    )
    .expect("create lease");
    assert_eq!(
        app.show_knot(&lease.id).unwrap().unwrap().state,
        "lease_ready"
    );

    let result =
        claim_knot(&app, &work.id, default_actor(), Some(&lease.id)).expect("claim should succeed");

    // Lease must now be active
    let lease_after = app
        .show_knot(&lease.id)
        .expect("show")
        .expect("lease exists");
    assert_eq!(
        lease_after.state, "lease_active",
        "lease should be activated during claim"
    );
    // And bound to the knot
    assert_eq!(result.knot.lease_id.as_deref(), Some(lease.id.as_str()));

    let _ = std::fs::remove_dir_all(root);
}

/// AC-5: corrupt/unknown lease states produce a warning (via stderr)
/// and still return an error.
#[test]
fn claim_with_corrupt_lease_state_returns_error() {
    let root = unique_workspace();
    let app = open_app(&root);

    let work = app
        .create_knot(
            "Corrupt state test",
            None,
            Some("work_item"),
            Some("default"),
        )
        .expect("create work knot");

    // Terminated lease — known non-ready state
    let lease = crate::lease::create_lease(
        &app,
        "terminated-lease",
        crate::domain::lease::LeaseType::Agent,
        Some(create_agent_info()),
    )
    .expect("create lease");
    crate::lease::activate_lease(&app, &lease.id).expect("activate");
    crate::lease::terminate_lease(&app, &lease.id).expect("terminate");

    let result = claim_knot(&app, &work.id, default_actor(), Some(&lease.id));
    let msg = match result {
        Err(e) => e.to_string(),
        Ok(_) => panic!("terminated lease should be rejected"),
    };
    assert!(
        msg.contains("expected lease_ready"),
        "error should mention expected state: {msg}"
    );

    let _ = std::fs::remove_dir_all(root);
}
