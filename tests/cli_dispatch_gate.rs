mod cli_dispatch_helpers;

use cli_dispatch_helpers::*;
use serde_json::Value;

fn create_gate_target(root: &std::path::Path, db: &std::path::Path) -> String {
    let target = run_knots(
        root,
        db,
        &[
            "new",
            "Blocked work",
            "--state",
            "shipped",
            "--profile",
            "autopilot",
        ],
    );
    assert_success(&target);
    parse_created_id(&target)
}

fn create_gate_knot(root: &std::path::Path, db: &std::path::Path, target_id: &str) -> String {
    let gate = run_knots(
        root,
        db,
        &[
            "new",
            "Release gate",
            "--type",
            "gate",
            "--gate-owner-kind",
            "human",
            "--gate-failure-mode",
            &format!("release blocked={target_id}"),
        ],
    );
    assert_success(&gate);
    let gate_id = parse_created_id(&gate);

    let gate_update = run_knots(
        root,
        db,
        &[
            "update",
            &gate_id,
            "--add-invariant",
            "State:release blocked",
        ],
    );
    assert_success(&gate_update);
    gate_id
}

#[test]
fn gate_knots_support_human_evaluation_and_reopen_flow() {
    let root_ws = unique_workspace("knots-cli-gate");
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let target_id = create_gate_target(&root, &db);
    let gate_id = create_gate_knot(&root, &db, &target_id);

    verify_gate_show(&root, &db, &gate_id);
    verify_gate_poll_and_claim(&root, &db, &gate_id);
    verify_gate_evaluate_and_reopen(&root, &db, &gate_id, &target_id);
}

fn verify_gate_show(root: &std::path::Path, db: &std::path::Path, gate_id: &str) {
    let shown = run_knots(root, db, &["show", gate_id, "--json"]);
    assert_success(&shown);
    let json: Value = serde_json::from_slice(&shown.stdout).expect("show json");
    assert_eq!(json["type"], "gate");
    assert_eq!(json["gate"]["owner_kind"], "human");
}

fn verify_gate_poll_and_claim(root: &std::path::Path, db: &std::path::Path, gate_id: &str) {
    let ready = run_knots(
        root,
        db,
        &["ready", "evaluate", "--owner", "human", "--json"],
    );
    assert_success(&ready);
    let ready_json: Value = serde_json::from_slice(&ready.stdout).expect("ready json");
    let ready_items = ready_json
        .as_array()
        .expect("ready json should be an array");
    assert_eq!(ready_items.len(), 1);
    let ready_id = ready_items[0]["id"]
        .as_str()
        .expect("ready item id should be a string");
    assert!(
        ready_id.ends_with(gate_id),
        "full id '{ready_id}' should end with '{gate_id}'"
    );

    let ready_text = run_knots(root, db, &["ready", "evaluate", "--owner", "human"]);
    assert_success(&ready_text);
    let ready_stdout = String::from_utf8_lossy(&ready_text.stdout);
    assert!(ready_stdout.contains("Release gate"));
    assert!(ready_stdout.contains("human -> evaluating"));

    let poll = run_knots(
        root,
        db,
        &["poll", "evaluate", "--owner", "human", "--json"],
    );
    assert_success(&poll);
    let poll_json: Value = serde_json::from_slice(&poll.stdout).expect("poll json");
    assert_eq!(poll_json["title"], "Release gate");

    let claim = run_knots(root, db, &["claim", gate_id, "--json"]);
    assert_success(&claim);
    let claim_json: Value = serde_json::from_slice(&claim.stdout).expect("claim json");
    assert_eq!(claim_json["state"], "evaluating");
    assert!(claim_json["prompt"]
        .as_str()
        .expect("prompt should exist")
        .contains("# Evaluating"));
}

fn verify_gate_evaluate_and_reopen(
    root: &std::path::Path,
    db: &std::path::Path,
    gate_id: &str,
    target_id: &str,
) {
    let evaluate = run_knots(
        root,
        db,
        &[
            "gate",
            "evaluate",
            gate_id,
            "--decision",
            "no",
            "--invariant",
            "release blocked",
            "--json",
        ],
    );
    assert_success(&evaluate);
    let json: Value = serde_json::from_slice(&evaluate.stdout).expect("evaluate json");
    assert_eq!(json["decision"], "no");
    assert_eq!(json["gate"]["state"], "abandoned");
    let reopened_id = json["reopened"][0]
        .as_str()
        .expect("reopened id should be a string");
    assert!(
        reopened_id.ends_with(target_id),
        "full id '{reopened_id}' should end with '{target_id}'"
    );

    let reopened = run_knots(root, db, &["show", target_id, "--json"]);
    assert_success(&reopened);
    let reopened_json: Value = serde_json::from_slice(&reopened.stdout).expect("show json");
    assert_eq!(reopened_json["state"], "ready_for_planning");
    assert!(reopened_json["notes"][0]
        .as_object()
        .and_then(|obj| obj.get("content"))
        .and_then(Value::as_str)
        .expect("note content")
        .contains("reopened this knot for planning"));
}
