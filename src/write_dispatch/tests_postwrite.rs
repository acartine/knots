//! Regressions for post-write failure handling on `kno next`.
//!
//! A lease-bound advance whose state write succeeds must exit successfully;
//! failures that occur after the write (profile resolution for unrelated
//! knots during lease release, display enrichment) are warnings, not errors.

use super::execute_operation;
use super::tests_lease_ext::{open_app, setup_repo, unique_workspace};
use crate::app::App;
use crate::poll_claim::{self, PollResult};
use crate::write_queue::{NextOperation, WriteOperation};

fn claim_default(app: &App, id: &str) -> PollResult {
    poll_claim::claim_knot(app, id, Some("agent".to_string()), None, 600, false).expect("claim")
}

fn next_op(id: &str, expected_state: &str, lease_id: Option<String>) -> WriteOperation {
    WriteOperation::Next(NextOperation {
        id: id.to_string(),
        expected_state: Some(expected_state.to_string()),
        json: false,
        approve_terminal_cascade: false,
        actor_kind: None,
        agent_name: None,
        agent_model: None,
        agent_version: None,
        lease_id,
    })
}

/// Point a bound knot's profile at a workflow/profile pair the registry
/// cannot resolve, simulating a store whose repo-config default workflow
/// references a missing profile.
fn corrupt_profile(app: &App, knot_id: &str) {
    app.conn_for_test()
        .execute(
            "UPDATE knot_hot SET workflow_id = 'quilt_sdlc', \
             profile_id = 'autopilot' WHERE id = ?1",
            [knot_id],
        )
        .expect("profile corruption should apply");
}

#[test]
fn next_succeeds_when_unrelated_bound_knot_has_unresolvable_profile() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    // An unrelated knot with a profile the registry cannot resolve, bound
    // to the same lease as the advancing knot. Before the fix, the
    // lease-release scan in `terminate_lease_and_recover_bound_action`
    // resolved every bound knot's profile and aborted the whole advance
    // on this record.
    let work = app
        .create_knot("Healthy advance", None, None, None)
        .expect("create work");
    let claimed = claim_default(&app, &work.id);
    let lease_id = claimed.knot.lease_id.clone().expect("bound lease");

    let unrelated = app
        .create_knot("Unrelated corrupt knot", None, None, None)
        .expect("create unrelated");
    crate::lease::bind_lease(&app, &unrelated.id, &lease_id).expect("bind");
    corrupt_profile(&app, &unrelated.id);

    let result = execute_operation(
        &app,
        &next_op(&work.id, &claimed.knot.state, Some(lease_id)),
    );
    assert!(
        result.is_ok(),
        "lease-bound next must succeed despite unrelated corrupt knot: {:?}",
        result.err()
    );

    let advanced = app.show_knot(&work.id).expect("show").expect("exists");
    assert_ne!(
        advanced.state, claimed.knot.state,
        "state should have advanced past the claimed action state"
    );
    assert!(
        advanced.lease_id.is_none(),
        "lease should be released after next"
    );
}

#[test]
fn next_succeeds_and_warns_when_lease_release_fails() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let work = app
        .create_knot("Release failure tolerated", None, None, None)
        .expect("create work");
    let claimed = claim_default(&app, &work.id);
    let lease_id = claimed.knot.lease_id.clone().expect("bound lease");

    // Two extra action-state knots bound to the same lease make the
    // release's recovery scan bail with "bound to multiple action-state
    // knots" — after the advance already committed. The advance must
    // still report success.
    for title in ["Extra action one", "Extra action two"] {
        let extra = app.create_knot(title, None, None, None).expect("create");
        crate::lease::bind_lease(&app, &extra.id, &lease_id).expect("bind");
        app.set_state_with_actor(
            &extra.id,
            "planning",
            true,
            None,
            crate::app::StateActorMetadata::default(),
        )
        .expect("force extra knot into an action state");
    }

    let result = execute_operation(
        &app,
        &next_op(&work.id, &claimed.knot.state, Some(lease_id)),
    );
    assert!(
        result.is_ok(),
        "next must succeed even when the lease release fails: {:?}",
        result.err()
    );
    let advanced = app.show_knot(&work.id).expect("show").expect("exists");
    assert_ne!(
        advanced.state, claimed.knot.state,
        "state should have advanced despite the failed release"
    );
}

#[test]
fn next_success_line_prints_when_write_succeeds() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let work = app
        .create_knot("Success output", None, None, None)
        .expect("create work");
    let claimed = claim_default(&app, &work.id);
    let lease_id = claimed.knot.lease_id.clone().expect("bound lease");

    let output = execute_operation(
        &app,
        &next_op(&work.id, &claimed.knot.state, Some(lease_id)),
    )
    .expect("next should succeed");
    assert!(
        output.contains("updated"),
        "success line should be printed: {output}"
    );
}

#[test]
fn next_without_lease_flag_names_bound_lease_id() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let work = app
        .create_knot("Guidance error", None, None, None)
        .expect("create work");
    let claimed = claim_default(&app, &work.id);
    let lease_id = claimed.knot.lease_id.clone().expect("bound lease");

    let err = execute_operation(&app, &next_op(&work.id, &claimed.knot.state, None))
        .expect_err("next without --lease must be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains(&lease_id),
        "guidance error should name the bound lease id '{lease_id}': {msg}"
    );
    assert!(
        msg.contains("--lease"),
        "guidance error should tell the caller to rerun with --lease: {msg}"
    );
}

#[test]
fn release_bound_lease_is_noop_without_lease() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let work = app
        .create_knot("No lease to release", None, None, None)
        .expect("create work");
    crate::lease_guard::release_bound_lease(&app, &work.id)
        .expect("release without a bound lease should be a no-op");
}

#[test]
fn release_bound_lease_rejects_non_lease_binding() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let work = app
        .create_knot("Corrupt binding", None, None, None)
        .expect("create work");
    let not_a_lease = app
        .create_knot("Plain work knot", None, None, None)
        .expect("create plain");
    crate::lease::bind_lease(&app, &work.id, &not_a_lease.id).expect("bind");

    let err = crate::lease_guard::release_bound_lease(&app, &work.id)
        .expect_err("binding to a non-lease knot must be rejected");
    assert!(
        err.to_string().contains("corrupt bound lease record"),
        "unexpected error: {err}"
    );
}
