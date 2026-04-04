mod cli_dispatch_helpers;

use cli_dispatch_helpers::*;
use serde_json::Value;

#[test]
fn claim_rejects_active_external_lease_with_warning_and_no_leak_id() {
    let root = unique_workspace("knots-cli-claim-active-lease");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Active external lease",
            "--profile",
            "autopilot",
            "--state",
            "ready_for_implementation",
        ],
    );
    assert_success(&created);
    let knot_id = parse_created_id(&created);

    let created_lease = run_knots(
        &root,
        &db,
        &["lease", "create", "--nickname", "active-ext", "--json"],
    );
    assert_success(&created_lease);
    let lease_json: Value =
        serde_json::from_slice(&created_lease.stdout).expect("lease json should parse");
    let lease_id = lease_json["id"].as_str().expect("lease id").to_string();

    assert_success(&run_knots(
        &root,
        &db,
        &["state", &lease_id, "lease_active"],
    ));

    let claim = run_knots(&root, &db, &["claim", &knot_id, "--lease", &lease_id]);
    assert_failure(&claim);
    let stderr = String::from_utf8_lossy(&claim.stderr);
    assert!(stderr.contains("warning: claim rejected external lease: lease_active"));
    assert!(
        stderr.contains("expected 'lease_ready'"),
        "stderr: {stderr}"
    );
    assert!(
        !stderr.contains(&lease_id),
        "stderr should not leak lease id: {stderr}"
    );

    let show = run_knots(&root, &db, &["show", &knot_id, "--json"]);
    assert_success(&show);
    let shown: Value = serde_json::from_slice(&show.stdout).expect("show json");
    assert_eq!(shown["state"], "ready_for_implementation");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn next_rejects_corrupt_bound_lease_with_warning_and_no_leak_id() {
    let root = unique_workspace("knots-cli-next-corrupt-lease");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Corrupt bound lease",
            "--profile",
            "autopilot",
            "--state",
            "ready_for_implementation",
        ],
    );
    assert_success(&created);
    let knot_id = parse_created_id(&created);

    let claim = run_knots(
        &root,
        &db,
        &["claim", &knot_id, "--agent-name", "test-agent", "--json"],
    );
    assert_success(&claim);
    let claim_json: Value = serde_json::from_slice(&claim.stdout).expect("claim json");
    let lease_id = claim_json["lease_id"]
        .as_str()
        .expect("lease id")
        .to_string();

    assert_success(&run_knots(
        &root,
        &db,
        &["state", &lease_id, "lease_terminated"],
    ));

    // An explicitly terminated lease is NOT covered by the expired-lease
    // exception. Only time-based expiry (raw!=terminated, effective==terminated)
    // allows kno next to succeed.
    let next = run_knots(
        &root,
        &db,
        &[
            "next",
            &knot_id,
            "--expected-state",
            "implementation",
            "--lease",
            &lease_id,
        ],
    );
    assert_failure(&next);

    let show = run_knots(&root, &db, &["show", &knot_id, "--json"]);
    assert_success(&show);
    let shown: Value = serde_json::from_slice(&show.stdout).expect("show json");
    assert_eq!(shown["state"], "implementation");

    let _ = std::fs::remove_dir_all(root);
}
