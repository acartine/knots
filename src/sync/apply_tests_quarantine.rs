//! Per-knot quarantine: a locally leased knot's events are held back
//! instead of applied, and replay in order once it is no longer leased.

use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use crate::db;
use crate::sync::GitAdapter;

use super::IncrementalApplier;

fn unique_workspace() -> knots_test_support::TestWorkspace {
    knots_test_support::workspace("knots-sync-apply-quarantine")
}

fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn setup_repo() -> knots_test_support::TestWorkspace {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    run_git(&root, &["init"]);
    run_git(&root, &["config", "user.email", "knots@example.com"]);
    run_git(&root, &["config", "user.name", "Knots Test"]);
    std::fs::write(root.join("README.md"), "# apply\n").expect("readme should be writable");
    run_git(&root, &["add", "README.md"]);
    run_git(&root, &["commit", "-m", "init"]);
    root_ws
}

fn open_conn(root: &Path) -> rusqlite::Connection {
    let db_path = root.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(db_path.parent().expect("db parent should exist"))
        .expect("db parent should be creatable");
    db::open_connection(db_path.to_str().expect("utf8 db path")).expect("db should open")
}

fn head_sha(root: &Path) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git rev-parse should run");
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn commit_all(root: &Path, message: &str) {
    run_git(root, &["add", ".knots"]);
    run_git(root, &["commit", "-m", message]);
}

fn write_index_event(
    root: &Path,
    event_id: &str,
    occurred_at: &str,
    knot_id: &str,
    title: &str,
    state: &str,
) {
    let dir = root.join(".knots/index/2026/04/01");
    std::fs::create_dir_all(&dir).expect("index dir should be creatable");
    let payload = serde_json::json!({
        "event_id": event_id,
        "occurred_at": occurred_at,
        "type": "idx.knot_head",
        "data": {
            "knot_id": knot_id,
            "title": title,
            "state": state,
            "workflow_id": "work_sdlc",
            "profile_id": "autopilot",
            "updated_at": occurred_at,
            "terminal": false
        }
    });
    std::fs::write(
        dir.join(format!("{event_id}-idx.knot_head.json")),
        payload.to_string(),
    )
    .expect("index event should write");
}

fn write_note_event(root: &Path, event_id: &str, occurred_at: &str, knot_id: &str, content: &str) {
    let dir = root.join(".knots/events/2026/04/01");
    std::fs::create_dir_all(&dir).expect("events dir should be creatable");
    let payload = serde_json::json!({
        "event_id": event_id,
        "occurred_at": occurred_at,
        "knot_id": knot_id,
        "type": "knot.note_added",
        "data": {
            "entry_id": event_id,
            "content": content,
            "username": "tester",
            "datetime": occurred_at,
            "agentname": "codex",
            "model": "gpt-5",
            "version": "1"
        }
    });
    std::fs::write(
        dir.join(format!("{event_id}-knot.note_added.json")),
        payload.to_string(),
    )
    .expect("note event should write");
}

fn leased(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|id| id.to_string()).collect()
}

#[test]
fn quarantine_holds_a_leased_knot_across_several_pulls_then_replays_it_once_unlocked() {
    let root_ws = setup_repo();
    let root = root_ws.path().to_path_buf();
    let conn = open_conn(&root);
    db::set_meta(&conn, "hot_window_days", "365").expect("hot window should be configurable");

    // Pull 1: K-A and K-B both get a first event; K-A is locally leased.
    write_index_event(
        &root,
        "1000",
        "2026-04-01T10:00:00Z",
        "K-A",
        "A v1",
        "work_item",
    );
    write_index_event(
        &root,
        "1001",
        "2026-04-01T10:00:01Z",
        "K-B",
        "B v1",
        "work_item",
    );
    commit_all(&root, "seed A and B");
    let head1 = head_sha(&root);

    let mut applier = IncrementalApplier::new_with_builtins_and_leased(
        &conn,
        root.clone(),
        GitAdapter::new(),
        leased(&["K-A"]),
    );
    let summary1 = applier
        .apply_to_head(&head1)
        .expect("first pull should succeed");
    assert_eq!(summary1.held_back_knots, vec!["K-A".to_string()]);
    assert!(
        db::get_knot_hot(&conn, "K-A")
            .expect("query should succeed")
            .is_none(),
        "the leased knot's event must not apply"
    );
    assert_eq!(
        db::get_knot_hot(&conn, "K-B")
            .expect("query should succeed")
            .expect("B should apply")
            .title,
        "B v1"
    );
    let rows = db::list_quarantined_knots(&conn).expect("list should succeed");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].knot_id, "K-A");
    let base_commit_after_first_skip = rows[0].base_commit.clone();

    // Pull 2: another event for K-A lands while it is still leased. The
    // quarantine's base commit must stay pinned to the first skip, or the
    // v1 event would be lost once the shared watermark moves past it.
    write_index_event(
        &root,
        "1002",
        "2026-04-01T10:05:00Z",
        "K-A",
        "A v2",
        "work_item",
    );
    commit_all(&root, "A v2");
    let head2 = head_sha(&root);

    let mut applier = IncrementalApplier::new_with_builtins_and_leased(
        &conn,
        root.clone(),
        GitAdapter::new(),
        leased(&["K-A"]),
    );
    let summary2 = applier
        .apply_to_head(&head2)
        .expect("second pull should succeed");
    assert_eq!(summary2.held_back_knots, vec!["K-A".to_string()]);
    assert!(db::get_knot_hot(&conn, "K-A")
        .expect("query should succeed")
        .is_none());
    let rows = db::list_quarantined_knots(&conn).expect("list should succeed");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].base_commit, base_commit_after_first_skip,
        "the base commit must not move forward while still quarantined"
    );

    // Pull 3: the lease is gone. Both A v1 and A v2 replay, in order,
    // converging on A v2 -- the same result an unfiltered sync would reach.
    let mut applier = IncrementalApplier::new_with_builtins_and_leased(
        &conn,
        root.clone(),
        GitAdapter::new(),
        HashSet::new(),
    );
    let summary3 = applier
        .apply_to_head(&head2)
        .expect("third pull should succeed");
    assert!(summary3.held_back_knots.is_empty());
    let record = db::get_knot_hot(&conn, "K-A")
        .expect("query should succeed")
        .expect("A should now be applied");
    assert_eq!(record.title, "A v2");
    assert!(db::list_quarantined_knots(&conn)
        .expect("list should succeed")
        .is_empty());
}

#[test]
fn full_events_for_a_leased_knot_are_quarantined_too() {
    let root_ws = setup_repo();
    let root = root_ws.path().to_path_buf();
    let conn = open_conn(&root);
    db::set_meta(&conn, "hot_window_days", "365").expect("hot window should be configurable");

    write_index_event(
        &root,
        "2000",
        "2026-04-01T11:00:00Z",
        "K-C",
        "C v1",
        "work_item",
    );
    commit_all(&root, "seed C");
    let head1 = head_sha(&root);
    let mut applier = IncrementalApplier::new_with_builtins(&conn, root.clone(), GitAdapter::new());
    applier
        .apply_to_head(&head1)
        .expect("first pull should apply C");
    assert!(db::get_knot_hot(&conn, "K-C")
        .expect("query should succeed")
        .is_some());

    // K-C becomes locally leased, and a note lands for it.
    write_note_event(&root, "2001", "2026-04-01T11:05:00Z", "K-C", "in progress");
    commit_all(&root, "note on C");
    let head2 = head_sha(&root);

    let mut applier = IncrementalApplier::new_with_builtins_and_leased(
        &conn,
        root.clone(),
        GitAdapter::new(),
        leased(&["K-C"]),
    );
    let summary = applier
        .apply_to_head(&head2)
        .expect("second pull should succeed");
    assert_eq!(summary.held_back_knots, vec!["K-C".to_string()]);
    let record = db::get_knot_hot(&conn, "K-C")
        .expect("query should succeed")
        .expect("C should still exist");
    assert!(
        record.notes.is_empty(),
        "the note must not apply while K-C is locally leased"
    );

    // Unlock: the note replays.
    let mut applier = IncrementalApplier::new_with_builtins_and_leased(
        &conn,
        root.clone(),
        GitAdapter::new(),
        HashSet::new(),
    );
    applier
        .apply_to_head(&head2)
        .expect("third pull should succeed");
    let record = db::get_knot_hot(&conn, "K-C")
        .expect("query should succeed")
        .expect("C should still exist");
    assert_eq!(record.notes.len(), 1);
    assert_eq!(record.notes[0].content, "in progress");
}
