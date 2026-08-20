mod cli_dispatch_helpers;

use cli_dispatch_helpers::*;
use serde_json::Value;

#[test]
fn verification_steps_cli_flags_round_trip_through_show_json() {
    let root_ws = unique_workspace("knots-cli-verification-steps");
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Verified knot",
            "--verification-step",
            "cargo test",
            "--verification-step",
            "make sanity",
        ],
    );
    assert_success(&created);
    let knot_id = parse_created_id(&created);

    let show = run_knots(&root, &db, &["show", &knot_id, "--json"]);
    assert_success(&show);
    let shown: Value = serde_json::from_slice(&show.stdout).expect("show json");
    assert_eq!(
        shown["verification_steps"],
        serde_json::json!(["cargo test", "make sanity"])
    );

    assert_success(&run_knots(
        &root,
        &db,
        &[
            "update",
            &knot_id,
            "--add-verification-step",
            "kno show --json",
        ],
    ));
    assert_success(&run_knots(
        &root,
        &db,
        &[
            "update",
            &knot_id,
            "--remove-verification-step",
            "make sanity",
        ],
    ));
    let show = run_knots(&root, &db, &["show", &knot_id, "--json"]);
    assert_success(&show);
    let shown: Value = serde_json::from_slice(&show.stdout).expect("show json");
    assert_eq!(
        shown["verification_steps"],
        serde_json::json!(["cargo test", "kno show --json"])
    );

    assert_success(&run_knots(
        &root,
        &db,
        &["update", &knot_id, "--clear-verification-steps"],
    ));
    let show = run_knots(&root, &db, &["show", &knot_id, "--json"]);
    assert_success(&show);
    let shown: Value = serde_json::from_slice(&show.stdout).expect("show json");
    assert_eq!(shown["verification_steps"], serde_json::json!([]));
}
