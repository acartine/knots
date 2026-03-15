use std::path::{Path, PathBuf};

use uuid::Uuid;

use super::{claim_knot, list_queue_candidates, run_poll};
use crate::app::{App, StateActorMetadata};
use crate::cli::PollArgs;
use crate::domain::knot_type::KnotType;

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-poll-lease-ext-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    root
}

fn open_app(root: &Path) -> App {
    let db_path = root.join(".knots/cache/state.sqlite");
    App::open(db_path.to_str().expect("utf8 db path"), root.to_path_buf()).expect("app should open")
}

#[test]
fn lease_excluded_from_queue_candidates() {
    let root = unique_workspace();
    let app = open_app(&root);

    // Create a lease knot (starts in lease_ready which is a queue state)
    let lease = crate::lease::create_lease(
        &app,
        "test-lease",
        crate::domain::lease::LeaseType::Manual,
        None,
    )
    .expect("lease should be created");
    assert_eq!(lease.knot_type, KnotType::Lease);

    // Create a regular work knot for contrast
    app.create_knot("Work item", None, Some("work_item"), Some("default"))
        .expect("work knot should be created");

    let candidates = list_queue_candidates(&app, None).expect("list should succeed");

    // The lease should not appear in candidates
    assert!(
        !candidates.iter().any(|k| k.id == lease.id),
        "lease should be excluded from queue candidates"
    );

    // The work knot should appear
    assert!(
        candidates.iter().any(|k| k.knot_type == KnotType::Work),
        "work item should be in queue candidates"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn claim_rejects_lease_knot() {
    let root = unique_workspace();
    let app = open_app(&root);

    let lease = crate::lease::create_lease(
        &app,
        "unclaimed-lease",
        crate::domain::lease::LeaseType::Manual,
        None,
    )
    .expect("lease should be created");

    let actor = StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: Some("test-agent".to_string()),
        agent_model: None,
        agent_version: None,
    };

    let result = claim_knot(&app, &lease.id, actor);
    let err = match result {
        Err(e) => e.to_string(),
        Ok(_) => panic!("claim should reject lease knot"),
    };
    assert!(
        err.contains("is a lease and cannot be claimed"),
        "error should mention lease rejection: {err}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn claim_creates_lease_on_claim() {
    let root = unique_workspace();
    let app = open_app(&root);

    let work = app
        .create_knot("Claimable work", None, Some("work_item"), Some("default"))
        .expect("work knot should be created");

    let actor = StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: Some("claude".to_string()),
        agent_model: Some("opus".to_string()),
        agent_version: Some("4".to_string()),
    };

    let _result = claim_knot(&app, &work.id, actor).expect("claim should succeed");
    let knot = app
        .show_knot(&work.id)
        .expect("show should succeed")
        .expect("knot should exist");

    assert!(
        knot.lease_id.is_some(),
        "claimed knot should have a lease_id"
    );

    // Verify the lease knot exists and is active
    let lease_id = knot.lease_id.as_ref().unwrap();
    let lease = app
        .show_knot(lease_id)
        .expect("show lease should succeed")
        .expect("lease knot should exist");
    assert_eq!(lease.knot_type, KnotType::Lease);
    assert_eq!(lease.state, "lease_active");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn claim_without_agent_name_skips_lease_creation() {
    let root = unique_workspace();
    let app = open_app(&root);

    let work = app
        .create_knot(
            "No agent name work",
            None,
            Some("work_item"),
            Some("default"),
        )
        .expect("work knot should be created");

    let actor = StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: None,
        agent_model: None,
        agent_version: None,
    };

    claim_knot(&app, &work.id, actor).expect("claim should succeed");
    let knot = app
        .show_knot(&work.id)
        .expect("show should succeed")
        .expect("knot should exist");

    assert!(
        knot.lease_id.is_none(),
        "claim without agent_name should not create a lease"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn run_poll_with_claim_creates_lease() {
    let root = unique_workspace();
    let app = open_app(&root);

    app.create_knot("Poll claim work", None, Some("work_item"), Some("default"))
        .expect("work knot should be created");

    let args = PollArgs {
        stage: None,
        owner: None,
        claim: true,
        json: true,
        agent_name: Some("test-agent".to_string()),
        agent_model: Some("test-model".to_string()),
        agent_version: Some("1.0".to_string()),
    };

    run_poll(&app, args).expect("run_poll with claim should succeed");

    let _ = std::fs::remove_dir_all(root);
}
