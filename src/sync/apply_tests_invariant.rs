use std::path::Path;
use std::process::Command;

use serde_json::json;
use uuid::Uuid;

use crate::db::{self, UpsertKnotHot};
use crate::domain::invariant::InvariantType;
use crate::sync::GitAdapter;

use super::IncrementalApplier;

fn unique_workspace() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("knots-apply-inv-{}", Uuid::now_v7()));
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

fn setup_repo() -> std::path::PathBuf {
    let root = unique_workspace();
    run_git(&root, &["init"]);
    run_git(&root, &["config", "user.email", "knots@example.com"]);
    run_git(&root, &["config", "user.name", "Knots Test"]);
    std::fs::write(root.join("README.md"), "# apply inv\n").expect("readme should be writable");
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

fn seed_hot_knot(conn: &rusqlite::Connection, knot_id: &str) {
    db::upsert_knot_hot(
        conn,
        &UpsertKnotHot {
            id: knot_id,
            title: "Seed",
            state: "implementation",
            updated_at: "2026-02-25T10:00:00Z",
            body: None,
            description: None,
            acceptance: None,
            priority: None,
            knot_type: None,
            tags: &[],
            notes: &[],
            handoff_capsules: &[],
            invariants: &[],
            step_history: &[],
            gate_data: &crate::domain::gate::GateData::default(),
            lease_data: &crate::domain::lease::LeaseData::default(),
            lease_id: None,
            workflow_id: "compatibility",
            profile_id: "autopilot",
            profile_etag: Some("etag-1"),
            deferred_from_state: None,
            blocked_from_state: None,
            created_at: Some("2026-02-25T10:00:00Z"),
        },
    )
    .expect("hot knot should upsert");
}

#[test]
fn apply_full_event_invariants_set_updates_hot_knot() {
    let root = setup_repo();
    let conn = open_conn(&root);
    seed_hot_knot(&conn, "K-inv");
    let applier = IncrementalApplier::new(&conn, root.clone(), GitAdapter::new());

    let events_dir = root.join(".knots/events/2026/03/05");
    std::fs::create_dir_all(&events_dir).expect("events directory should be creatable");

    let inv_path = events_dir.join("7000-knot.invariants_set.json");
    let payload = json!({
        "event_id": "7000",
        "occurred_at": "2026-03-05T10:00:00Z",
        "knot_id": "K-inv",
        "type": "knot.invariants_set",
        "data": {
            "invariants": [
                {"type": "Scope", "condition": "only src/db.rs"},
                {"type": "State", "condition": "no regressions"}
            ]
        }
    });
    std::fs::write(&inv_path, payload.to_string()).expect("invariants_set event should write");

    applier
        .apply_full_event(Path::new(
            ".knots/events/2026/03/05/\
             7000-knot.invariants_set.json",
        ))
        .expect("invariants_set event should apply");

    let record = db::get_knot_hot(&conn, "K-inv")
        .expect("hot lookup should succeed")
        .expect("hot knot should exist");
    assert_eq!(record.invariants.len(), 2);
    assert_eq!(record.invariants[0].invariant_type, InvariantType::Scope);
    assert_eq!(record.invariants[0].condition, "only src/db.rs");
    assert_eq!(record.invariants[1].invariant_type, InvariantType::State);
    assert_eq!(record.invariants[1].condition, "no regressions");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn apply_index_event_with_invariants_persists_them() {
    let root = setup_repo();
    let conn = open_conn(&root);
    db::set_meta(&conn, "hot_window_days", "365").expect("hot window should be configurable");
    let applier = IncrementalApplier::new(&conn, root.clone(), GitAdapter::new());

    let idx_dir = root.join(".knots/index/2026/03/05");
    std::fs::create_dir_all(&idx_dir).expect("index directory should be creatable");

    let idx_path = idx_dir.join("7001-idx.knot_head.json");
    let now = time::OffsetDateTime::now_utc();
    let ts = now
        .format(&time::format_description::well_known::Rfc3339)
        .expect("timestamp should format");
    let payload = json!({
        "event_id": "7001",
        "occurred_at": ts,
        "type": "idx.knot_head",
        "data": {
            "knot_id": "K-idx-inv",
            "title": "Index with invariants",
            "state": "implementation",
            "profile_id": "autopilot",
            "updated_at": ts,
            "terminal": false,
            "invariants": [
                {
                    "type": "Scope",
                    "condition": "src/ only"
                }
            ]
        }
    });
    std::fs::write(&idx_path, payload.to_string()).expect("index event should write");

    let updated = applier
        .apply_index_event(Path::new(
            ".knots/index/2026/03/05/\
             7001-idx.knot_head.json",
        ))
        .expect("index event should apply");
    assert!(updated);

    let record = db::get_knot_hot(&conn, "K-idx-inv")
        .expect("hot lookup should succeed")
        .expect("hot knot should exist");
    assert_eq!(record.invariants.len(), 1);
    assert_eq!(record.invariants[0].invariant_type, InvariantType::Scope);
    assert_eq!(record.invariants[0].condition, "src/ only");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn apply_invariants_set_on_missing_hot_knot_is_noop() {
    let root = setup_repo();
    let conn = open_conn(&root);
    let applier = IncrementalApplier::new(&conn, root.clone(), GitAdapter::new());

    let events_dir = root.join(".knots/events/2026/03/05");
    std::fs::create_dir_all(&events_dir).expect("events directory should be creatable");

    let inv_path = events_dir.join("7002-knot.invariants_set.json");
    let payload = json!({
        "event_id": "7002",
        "occurred_at": "2026-03-05T10:00:00Z",
        "knot_id": "K-ghost",
        "type": "knot.invariants_set",
        "data": {
            "invariants": [
                {"type": "State", "condition": "noop test"}
            ]
        }
    });
    std::fs::write(&inv_path, payload.to_string()).expect("invariants_set event should write");

    applier
        .apply_full_event(Path::new(
            ".knots/events/2026/03/05/\
             7002-knot.invariants_set.json",
        ))
        .expect("invariants_set on missing knot is noop");

    let record = db::get_knot_hot(&conn, "K-ghost").expect("hot lookup should succeed");
    assert!(record.is_none());

    let _ = std::fs::remove_dir_all(root);
}
