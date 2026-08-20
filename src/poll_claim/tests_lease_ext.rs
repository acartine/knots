use std::path::Path;

use super::{claim_knot, list_queue_candidates, run_poll};
use crate::app::App;
use crate::cli::PollArgs;
use crate::domain::knot_type::KnotType;

pub(super) fn unique_workspace() -> knots_test_support::TestWorkspace {
    knots_test_support::workspace("knots-poll-lease-ext")
}

pub(super) fn open_app(root: &Path) -> App {
    let db_path = root.join(".knots/cache/state.sqlite");
    App::open(db_path.to_str().expect("utf8 db path"), root.to_path_buf()).expect("app should open")
}

#[test]
fn lease_excluded_from_queue_candidates() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let app = open_app(&root);

    // Create a lease knot (starts in lease_ready which is a queue state)
    let lease = crate::lease::create_lease(
        &app,
        "test-lease",
        crate::domain::lease::LeaseType::Manual,
        None,
        600,
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
}

#[test]
fn claim_rejects_lease_knot() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let app = open_app(&root);

    let lease = crate::lease::create_lease(
        &app,
        "unclaimed-lease",
        crate::domain::lease::LeaseType::Manual,
        None,
        600,
    )
    .expect("lease should be created");

    let result = claim_knot(&app, &lease.id, Some("agent".to_string()), None, 600, false);
    let err = match result {
        Err(e) => e.to_string(),
        Ok(_) => panic!("claim should reject lease knot"),
    };
    assert!(
        err.contains("is a lease and cannot be claimed"),
        "error should mention lease rejection: {err}"
    );
}

#[test]
fn claim_creates_lease_on_claim() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let app = open_app(&root);

    let work = app
        .create_knot("Claimable work", None, Some("work_item"), Some("default"))
        .expect("work knot should be created");

    let _result = claim_knot(&app, &work.id, Some("agent".to_string()), None, 600, false)
        .expect("claim should succeed");
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
}

#[test]
fn claim_without_external_lease_always_auto_creates_one() {
    // With lease-declared identity, `kno claim` (no `--lease`) must always
    // auto-create a lease. The CLI no longer has a path for declaring
    // agent_name, so the auto-created lease simply has an empty agent_info.
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let app = open_app(&root);

    let work = app
        .create_knot("Auto-lease work", None, Some("work_item"), Some("default"))
        .expect("work knot should be created");

    claim_knot(&app, &work.id, Some("agent".to_string()), None, 600, false)
        .expect("claim should succeed");
    let knot = app
        .show_knot(&work.id)
        .expect("show should succeed")
        .expect("knot should exist");

    assert!(
        knot.lease_id.is_some(),
        "claim without --lease should auto-create one"
    );
}

#[test]
fn run_poll_with_claim_creates_lease() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let app = open_app(&root);

    app.create_knot("Poll claim work", None, Some("work_item"), Some("default"))
        .expect("work knot should be created");

    let args = PollArgs {
        stage: None,
        owner: None,
        claim: true,
        json: true,
        // Deprecated agent-identity flags — still parseable but ignored.
        agent_name: None,
        agent_model: None,
        agent_version: None,
        timeout_seconds: None,
        e2e: false,
    };

    run_poll(&app, args).expect("run_poll with claim should succeed");
}

pub(super) fn setup_repo(root: &std::path::Path) {
    use std::process::Command;
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("git should run");
        assert!(output.status.success(), "git {:?} failed", args);
    };
    run(&["init"]);
    run(&["config", "user.email", "knots@example.com"]);
    run(&["config", "user.name", "Knots Test"]);
    std::fs::write(root.join("README.md"), "# knots\n").expect("write readme");
    run(&["add", "README.md"]);
    run(&["commit", "-m", "init"]);
    run(&["branch", "-M", "main"]);
}

pub(super) fn create_agent_info() -> crate::domain::lease::AgentInfo {
    crate::domain::lease::AgentInfo {
        agent_type: "cli".to_string(),
        provider: "test".to_string(),
        agent_name: "test-agent".to_string(),
        model: "test-model".to_string(),
        model_version: "1.0".to_string(),
    }
}

#[test]
fn claim_with_external_lease_binds_it() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let app = open_app(&root);

    let work = app
        .create_knot(
            "External lease test",
            None,
            Some("work_item"),
            Some("default"),
        )
        .expect("create work knot");

    let lease = crate::lease::create_lease(
        &app,
        "test-external-lease",
        crate::domain::lease::LeaseType::Agent,
        Some(create_agent_info()),
        600,
    )
    .expect("create lease");

    let result = claim_knot(
        &app,
        &work.id,
        Some("agent".to_string()),
        Some(&lease.id),
        600,
        false,
    )
    .expect("claim should succeed");

    // Verify the external lease is bound (not a new one)
    assert_eq!(result.knot.lease_id.as_deref(), Some(lease.id.as_str()));
}

#[test]
fn claim_with_terminated_lease_rejects() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let app = open_app(&root);

    let work = app
        .create_knot(
            "Terminated lease test",
            None,
            Some("work_item"),
            Some("default"),
        )
        .expect("create work knot");

    let lease = crate::lease::create_lease(
        &app,
        "terminated-lease",
        crate::domain::lease::LeaseType::Agent,
        Some(create_agent_info()),
        600,
    )
    .expect("create lease");
    let _ = crate::lease::activate_lease(&app, &lease.id);
    let _ = crate::lease::terminate_lease(&app, &lease.id);

    let result = claim_knot(
        &app,
        &work.id,
        Some("agent".to_string()),
        Some(&lease.id),
        600,
        false,
    );
    assert!(
        result.is_err(),
        "claiming with terminated lease should fail"
    );
}

#[test]
fn claim_without_lease_creates_one() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let app = open_app(&root);

    let work = app
        .create_knot(
            "No external lease",
            None,
            Some("work_item"),
            Some("default"),
        )
        .expect("create work knot");

    let result = claim_knot(&app, &work.id, Some("agent".to_string()), None, 600, false)
        .expect("claim should succeed");

    assert!(
        result.knot.lease_id.is_some(),
        "should have auto-created a lease"
    );
}

#[test]
fn completion_command_includes_lease() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let app = open_app(&root);

    let work = app
        .create_knot(
            "Completion cmd test",
            None,
            Some("work_item"),
            Some("default"),
        )
        .expect("create knot");

    let lease = crate::lease::create_lease(
        &app,
        "test-completion-lease",
        crate::domain::lease::LeaseType::Agent,
        Some(create_agent_info()),
        600,
    )
    .expect("create lease");

    let result = claim_knot(
        &app,
        &work.id,
        Some("agent".to_string()),
        Some(&lease.id),
        600,
        false,
    )
    .expect("claim with external lease");

    assert!(
        result.completion_cmd.contains("--lease"),
        "completion command should include --lease flag"
    );
    assert!(
        result.completion_cmd.contains(&lease.id),
        "completion command should include the lease ID"
    );
}

#[test]
fn claim_with_non_lease_knot_as_lease_rejects() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let app = open_app(&root);

    let work = app
        .create_knot(
            "Non-lease external",
            None,
            Some("work_item"),
            Some("default"),
        )
        .expect("create work knot");

    // Create a second work knot and try to use it as a lease
    let fake_lease = app
        .create_knot("Not a lease", None, Some("work_item"), Some("default"))
        .expect("create second knot");

    let result = claim_knot(
        &app,
        &work.id,
        Some("agent".to_string()),
        Some(&fake_lease.id),
        600,
        false,
    );
    let err = match result {
        Err(e) => e.to_string(),
        Ok(_) => panic!("claiming with non-lease knot should fail"),
    };
    assert!(
        err.contains("does not point to a lease knot"),
        "error should mention not a lease: {err}"
    );
}

#[test]
fn claim_with_ready_lease_activates_it() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let app = open_app(&root);

    let work = app
        .create_knot("Ready lease test", None, Some("work_item"), Some("default"))
        .expect("create work knot");

    // Create a lease but do NOT activate it (stays in lease_ready)
    let lease = crate::lease::create_lease(
        &app,
        "ready-lease",
        crate::domain::lease::LeaseType::Agent,
        Some(create_agent_info()),
        600,
    )
    .expect("create lease");
    // Verify it starts in lease_ready
    let lease_knot = app
        .show_knot(&lease.id)
        .expect("show")
        .expect("lease exists");
    assert_eq!(lease_knot.state, "lease_ready");

    let result = claim_knot(
        &app,
        &work.id,
        Some("agent".to_string()),
        Some(&lease.id),
        600,
        false,
    )
    .expect("claim should succeed");

    // Verify the lease was activated and bound
    assert_eq!(result.knot.lease_id.as_deref(), Some(lease.id.as_str()));
    let lease_after = app
        .show_knot(&lease.id)
        .expect("show")
        .expect("lease exists");
    assert_eq!(lease_after.state, "lease_active");
}

#[test]
fn claim_with_nonexistent_lease_rejects() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let app = open_app(&root);

    let work = app
        .create_knot(
            "Nonexistent lease test",
            None,
            Some("work_item"),
            Some("default"),
        )
        .expect("create work knot");

    let result = claim_knot(
        &app,
        &work.id,
        Some("agent".to_string()),
        Some("nonexistent-lease-id"),
        600,
        false,
    );
    assert!(
        result.is_err(),
        "claiming with nonexistent lease should fail"
    );
}
