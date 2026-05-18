use std::path::{Path, PathBuf};
use std::process::Command;

use uuid::Uuid;

use crate::db;
use crate::sync::GitAdapter;

use super::IncrementalApplier;

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-sync-unknown-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    root
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

fn setup_repo() -> PathBuf {
    let root = unique_workspace();
    run_git(&root, &["init"]);
    run_git(&root, &["config", "user.email", "knots@example.com"]);
    run_git(&root, &["config", "user.name", "Knots Test"]);
    std::fs::write(root.join("README.md"), "# apply\n").expect("readme should be writable");
    run_git(&root, &["add", "README.md"]);
    run_git(&root, &["commit", "-m", "init"]);
    root
}

fn open_conn(root: &Path) -> rusqlite::Connection {
    let db_path = root.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(db_path.parent().expect("db parent should exist"))
        .expect("db parent should be creatable");
    db::open_connection(db_path.to_str().expect("utf8 db path")).expect("db should open")
}

fn recent_ts() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("timestamp should format")
}

fn write_unknown_workflow_head(root: &Path, knot_id: &str, filename: &str) -> PathBuf {
    let idx_dir = root.join(".knots/index/2026/02/25");
    std::fs::create_dir_all(&idx_dir).expect("index directory should be creatable");
    let ts = recent_ts();
    let payload = serde_json::json!({
        "event_id": "9000",
        "occurred_at": ts,
        "type": "idx.knot_head",
        "data": {
            "knot_id": knot_id,
            "title": "Future workflow",
            "state": "work_item",
            "workflow_id": "future_sdlc",
            "profile_id": "autopilot",
            "updated_at": ts,
            "terminal": false
        }
    });
    std::fs::write(idx_dir.join(filename), payload.to_string()).expect("index event should write");
    Path::new(".knots/index/2026/02/25").join(filename)
}

#[test]
fn apply_index_event_skips_unknown_workflow_with_import_warning() {
    let root = setup_repo();
    let conn = open_conn(&root);
    db::set_meta(&conn, "hot_window_days", "365").expect("hot window should be configurable");
    let mut applier = IncrementalApplier::new_with_builtins(&conn, root.clone(), GitAdapter::new());
    let rel_path = write_unknown_workflow_head(&root, "K-future", "9000-idx.knot_head.json");

    let updated = applier
        .apply_index_event(&rel_path)
        .expect("unknown workflow should skip, not fail");
    assert!(!updated);
    assert!(
        db::get_knot_hot(&conn, "K-future")
            .expect("hot lookup should succeed")
            .is_none(),
        "skipped knot should not be imported"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn apply_full_event_skips_knot_after_unknown_workflow_head() {
    let root = setup_repo();
    let conn = open_conn(&root);
    db::set_meta(&conn, "hot_window_days", "365").expect("hot window should be configurable");
    let mut applier = IncrementalApplier::new_with_builtins(&conn, root.clone(), GitAdapter::new());
    let head_path = write_unknown_workflow_head(&root, "K-future-full", "9001-idx.knot_head.json");

    let full_dir = root.join(".knots/events/2026/02/25");
    std::fs::create_dir_all(&full_dir).expect("full event directory should be creatable");
    let full_payload = serde_json::json!({
        "event_id": "9002",
        "occurred_at": recent_ts(),
        "knot_id": "K-future-full",
        "type": "knot.edge_add",
        "data": {
            "kind": "relates_to",
            "dst": "K-other"
        }
    });
    std::fs::write(
        full_dir.join("9002-knot.edge_add.json"),
        full_payload.to_string(),
    )
    .expect("full event should write");

    applier
        .apply_index_event(&head_path)
        .expect("unknown workflow should skip, not fail");
    let outcome = applier
        .apply_full_event(Path::new(
            ".knots/events/2026/02/25/9002-knot.edge_add.json",
        ))
        .expect("full event for skipped knot should not fail");
    assert!(matches!(outcome, super::FullApplyOutcome::Ignored));
    assert_eq!(
        db::list_edges(&conn, "K-future-full", db::EdgeDirection::Outgoing)
            .expect("edge lookup should succeed")
            .len(),
        0
    );

    let _ = std::fs::remove_dir_all(root);
}
