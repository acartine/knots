mod cli_dispatch_helpers;

use cli_dispatch_helpers::*;
use serde_json::Value;

#[test]
fn push_pull_and_sync_emit_progress_and_json() {
    let root = unique_workspace("knots-cli-sync-progress");
    let _remote = setup_repo_with_remote(&root);
    let db = root.join(".knots/cache/state.sqlite");

    assert_success(&run_knots(&root, &db, &["init"]));
    assert_success(&run_knots(&root, &db, &["new", "Progress knot"]));

    let push = run_knots(&root, &db, &["push"]);
    assert_success(&push);
    assert_contains_in_order(
        &String::from_utf8_lossy(&push.stdout),
        &[
            "publishing local knots events",
            "preparing knots worktree",
            "scanning local knots event files",
            "copying",
            "pushing knots branch to origin",
            "push local_event_files=",
        ],
    );

    let pull = run_knots(&root, &db, &["pull"]);
    assert_success(&pull);
    assert_contains_in_order(
        &String::from_utf8_lossy(&pull.stdout),
        &[
            "importing knots updates",
            "preparing knots worktree",
            "applying knots events to the local cache",
            "pull head=",
        ],
    );

    let sync = run_knots(&root, &db, &["sync"]);
    assert_success(&sync);
    assert_contains_in_order(
        &String::from_utf8_lossy(&sync.stdout),
        &[
            "publishing local knots events",
            "importing knots updates",
            "sync push(",
        ],
    );

    let push_json = run_knots(&root, &db, &["push", "--json"]);
    assert_success(&push_json);
    assert!(!String::from_utf8_lossy(&push_json.stdout).contains("publishing knots events"));
    let _: Value = serde_json::from_slice(&push_json.stdout).expect("push --json should parse");

    let pull_json = run_knots(&root, &db, &["pull", "--json"]);
    assert_success(&pull_json);
    assert!(!String::from_utf8_lossy(&pull_json.stdout).contains("importing knots updates"));
    let _: Value = serde_json::from_slice(&pull_json.stdout).expect("pull --json should parse");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn pull_warns_when_local_drift_exceeds_threshold() {
    let root = unique_workspace("knots-cli-pull-drift-warning");
    let _remote = setup_repo_with_remote(&root);
    let db = root.join(".knots/cache/state.sqlite");

    assert_success(&run_knots(&root, &db, &["init-remote"]));
    assert_success(&run_knots(
        &root,
        &db,
        &[
            "new",
            "Drift warning knot",
            "--profile",
            "autopilot",
            "--state",
            "idea",
        ],
    ));

    set_meta_value(&db, "pull_drift_warn_threshold", "1");

    let pull = run_knots(&root, &db, &["pull"]);
    assert_success(&pull);
    let stderr = String::from_utf8_lossy(&pull.stderr);
    assert!(
        stderr.contains("warning: local knots drift is high"),
        "pull warning: {stderr}"
    );
    assert!(stderr.contains("run `kno push`"), "pull warning: {stderr}");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cli_dispatch_covers_json_branches_and_cold_search_results() {
    let root = unique_workspace("knots-cli-json-branches");
    let _remote = setup_repo_with_remote(&root);
    let db = root.join(".knots/cache/state.sqlite");

    assert_success(&run_knots(&root, &db, &["init-remote"]));
    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Cold candidate",
            "--profile",
            "autopilot",
            "--state",
            "shipped",
        ],
    );
    assert_success(&created);
    let knot_id = parse_created_id(&created);

    assert_success(&run_knots(
        &root,
        &db,
        &[
            "update",
            &knot_id,
            "--description",
            "cold description",
            "--add-note",
            "note body",
            "--note-username",
            "acartine",
            "--note-datetime",
            "2026-02-25T10:00:00Z",
            "--note-agentname",
            "codex",
            "--note-model",
            "gpt-5",
            "--note-version",
            "1",
            "--add-handoff-capsule",
            "handoff body",
            "--handoff-username",
            "acartine",
            "--handoff-datetime",
            "2026-02-25T10:05:00Z",
            "--handoff-agentname",
            "codex",
            "--handoff-model",
            "gpt-5",
            "--handoff-version",
            "1",
        ],
    ));

    assert_success(&run_knots(&root, &db, &["push", "--json"]));
    assert_success(&run_knots(&root, &db, &["pull", "--json"]));
    assert_success(&run_knots(&root, &db, &["sync", "--json"]));
    assert_success(&run_knots(
        &root,
        &db,
        &["perf", "--iterations", "1", "--json"],
    ));
    assert_success(&run_knots(&root, &db, &["compact", "--write-snapshots"]));
    assert_success(&run_knots(&root, &db, &["cold", "sync", "--json"]));

    let cold_json = run_knots(&root, &db, &["cold", "search", "Cold", "--json"]);
    assert_success(&cold_json);
    let matches: Value = serde_json::from_slice(&cold_json.stdout).expect("cold search json");
    assert!(matches.as_array().is_some());

    let cold_text = run_knots(&root, &db, &["cold", "search", "Cold"]);
    assert_success(&cold_text);
    assert!(String::from_utf8_lossy(&cold_text.stdout).contains("Cold"));

    let _ = std::fs::remove_dir_all(root);
}
