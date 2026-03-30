mod cli_dispatch_helpers;

use cli_dispatch_helpers::*;
use serde_json::Value;

fn add_notes_and_handoffs(root: &std::path::Path, db: &std::path::Path, knot_id: &str) {
    assert_success(&run_knots(
        root,
        db,
        &[
            "update",
            knot_id,
            "--add-note",
            "old note",
            "--note-agentname",
            "agent-1",
        ],
    ));
    assert_success(&run_knots(
        root,
        db,
        &[
            "update",
            knot_id,
            "--add-note",
            "new note",
            "--note-agentname",
            "agent-2",
        ],
    ));
    assert_success(&run_knots(
        root,
        db,
        &[
            "update",
            knot_id,
            "--add-handoff-capsule",
            "old handoff",
            "--handoff-agentname",
            "agent-3",
        ],
    ));
    assert_success(&run_knots(
        root,
        db,
        &[
            "update",
            knot_id,
            "--add-handoff-capsule",
            "new handoff",
            "--handoff-agentname",
            "agent-4",
        ],
    ));
}

#[test]
fn show_hides_older_metadata_unless_verbose() {
    let root = unique_workspace("knots-cli-show-metadata");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let created = run_knots(
        &root,
        &db,
        &["new", "Show metadata", "--profile", "autopilot"],
    );
    assert_success(&created);
    let knot_id = parse_created_id(&created);
    add_notes_and_handoffs(&root, &db, &knot_id);

    assert_show_hides_old_metadata(&root, &db, &knot_id);
    assert_show_json_hides_old_metadata(&root, &db, &knot_id);
    assert_show_verbose_includes_all(&root, &db, &knot_id);

    let _ = std::fs::remove_dir_all(root);
}

fn assert_show_hides_old_metadata(root: &std::path::Path, db: &std::path::Path, knot_id: &str) {
    let show = run_knots(root, db, &["show", knot_id]);
    assert_success(&show);
    let stdout = String::from_utf8_lossy(&show.stdout);
    assert!(!stdout.contains("old note"), "show: {stdout}");
    assert!(stdout.contains("new note"), "show: {stdout}");
    assert!(!stdout.contains("old handoff"), "show: {stdout}");
    assert!(stdout.contains("new handoff"), "show: {stdout}");
    assert!(stdout.contains("1 older note"), "show: {stdout}");
    assert!(stdout.contains("1 older handoff capsule"), "show: {stdout}");
}

fn assert_show_json_hides_old_metadata(
    root: &std::path::Path,
    db: &std::path::Path,
    knot_id: &str,
) {
    let show_json = run_knots(root, db, &["show", knot_id, "--json"]);
    assert_success(&show_json);
    let json: Value = serde_json::from_slice(&show_json.stdout).expect("show json should parse");
    assert_eq!(json["notes"].as_array().unwrap().len(), 1);
    assert_eq!(json["notes"][0]["content"], "new note");
    assert_eq!(json["handoff_capsules"].as_array().unwrap().len(), 1);
    assert_eq!(json["handoff_capsules"][0]["content"], "new handoff");
    let other = json["other"].as_str().expect("other hint should exist");
    assert!(other.contains("1 older note"));
    assert!(other.contains("1 older handoff capsule"));
}

fn assert_show_verbose_includes_all(root: &std::path::Path, db: &std::path::Path, knot_id: &str) {
    let show = run_knots(root, db, &["show", knot_id, "--verbose"]);
    assert_success(&show);
    let stdout = String::from_utf8_lossy(&show.stdout);
    assert!(stdout.contains("old note"), "verbose: {stdout}");
    assert!(stdout.contains("new note"), "verbose: {stdout}");
    assert!(stdout.contains("old handoff"), "verbose: {stdout}");
    assert!(stdout.contains("new handoff"), "verbose: {stdout}");
    assert!(!stdout.contains("not shown"), "verbose: {stdout}");
}

fn add_simple_notes_and_handoffs(root: &std::path::Path, db: &std::path::Path, knot_id: &str) {
    assert_success(&run_knots(
        root,
        db,
        &["update", knot_id, "--add-note", "old note"],
    ));
    assert_success(&run_knots(
        root,
        db,
        &["update", knot_id, "--add-note", "new note"],
    ));
    assert_success(&run_knots(
        root,
        db,
        &["update", knot_id, "--add-handoff-capsule", "old handoff"],
    ));
    assert_success(&run_knots(
        root,
        db,
        &["update", knot_id, "--add-handoff-capsule", "new handoff"],
    ));
}

#[test]
fn claim_hides_older_metadata_unless_verbose() {
    let root = unique_workspace("knots-cli-claim-metadata");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Claim metadata",
            "--profile",
            "autopilot",
            "--state",
            "ready_for_implementation",
        ],
    );
    assert_success(&created);
    let knot_id = parse_created_id(&created);
    add_notes_and_handoffs(&root, &db, &knot_id);

    let claim = run_knots(&root, &db, &["claim", &knot_id]);
    assert_success(&claim);
    let stdout = String::from_utf8_lossy(&claim.stdout);
    assert!(!stdout.contains("old note"), "claim: {stdout}");
    assert!(stdout.contains("new note"), "claim: {stdout}");
    assert!(!stdout.contains("old handoff"), "claim: {stdout}");
    assert!(stdout.contains("new handoff"), "claim: {stdout}");
    assert!(stdout.contains("1 older note"), "claim: {stdout}");
    assert!(
        stdout.contains("1 older handoff capsule"),
        "claim: {stdout}"
    );

    assert_claim_json_hides_old(&root, &db);
    assert_claim_verbose_includes_all(&root, &db);

    let _ = std::fs::remove_dir_all(root);
}

fn assert_claim_json_hides_old(root: &std::path::Path, db: &std::path::Path) {
    let created = run_knots(
        root,
        db,
        &[
            "new",
            "Claim metadata json",
            "--profile",
            "autopilot",
            "--state",
            "ready_for_implementation",
        ],
    );
    assert_success(&created);
    let knot_id = parse_created_id(&created);
    add_simple_notes_and_handoffs(root, db, &knot_id);

    let claim = run_knots(root, db, &["claim", &knot_id, "--json"]);
    assert_success(&claim);
    let json: Value = serde_json::from_slice(&claim.stdout).expect("claim json should parse");
    let prompt = json["prompt"].as_str().expect("claim prompt exists");
    assert!(!prompt.contains("old note"));
    assert!(prompt.contains("new note"));
    assert!(!prompt.contains("old handoff"));
    assert!(prompt.contains("new handoff"));
    let other = json["other"].as_str().expect("other hint should exist");
    assert!(other.contains("1 older note"));
    assert!(other.contains("1 older handoff capsule"));
}

fn assert_claim_verbose_includes_all(root: &std::path::Path, db: &std::path::Path) {
    let created = run_knots(
        root,
        db,
        &[
            "new",
            "Claim metadata verbose",
            "--profile",
            "autopilot",
            "--state",
            "ready_for_implementation",
        ],
    );
    assert_success(&created);
    let knot_id = parse_created_id(&created);
    add_simple_notes_and_handoffs(root, db, &knot_id);

    let claim = run_knots(root, db, &["claim", &knot_id, "--verbose"]);
    assert_success(&claim);
    let stdout = String::from_utf8_lossy(&claim.stdout);
    assert!(stdout.contains("old note"), "verbose: {stdout}");
    assert!(stdout.contains("new note"), "verbose: {stdout}");
    assert!(stdout.contains("old handoff"), "verbose: {stdout}");
    assert!(stdout.contains("new handoff"), "verbose: {stdout}");
    assert!(!stdout.contains("not shown"), "verbose: {stdout}");
}
