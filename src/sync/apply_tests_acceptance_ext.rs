use std::path::{Path, PathBuf};
use std::process::Command;

use uuid::Uuid;

use crate::db::{self, UpsertKnotHot};
use crate::sync::GitAdapter;

use super::IncrementalApplier;

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-sync-acceptance-{}", Uuid::now_v7()));
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

#[test]
fn apply_full_event_updates_acceptance_metadata() {
    let root = setup_repo();
    let conn = open_conn(&root);
    db::upsert_knot_hot(
        &conn,
        &UpsertKnotHot {
            id: "K-1",
            title: "Seed",
            state: "ready_for_implementation",
            updated_at: "2026-03-22T10:00:00Z",
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
            workflow_id: "knots_sdlc",
            profile_id: "autopilot",
            profile_etag: Some("etag-1"),
            deferred_from_state: None,
            blocked_from_state: None,
            created_at: Some("2026-03-22T10:00:00Z"),
        },
    )
    .expect("seed knot should upsert");

    let event_path = root
        .join(".knots")
        .join("events")
        .join("2026")
        .join("03")
        .join("22")
        .join("0100-knot.acceptance_set.json");
    std::fs::create_dir_all(
        event_path
            .parent()
            .expect("event parent directory should exist"),
    )
    .expect("event parent should be creatable");
    std::fs::write(
        &event_path,
        concat!(
            "{\n",
            "  \"event_id\": \"0100\",\n",
            "  \"occurred_at\": \"2026-03-22T10:01:00Z\",\n",
            "  \"knot_id\": \"K-1\",\n",
            "  \"type\": \"knot.acceptance_set\",\n",
            "  \"data\": {\"acceptance\": \"Synced criteria\"}\n",
            "}\n"
        ),
    )
    .expect("event should write");

    let applier = IncrementalApplier::new(&conn, root.clone(), GitAdapter::new());
    applier
        .apply_full_event(Path::new(
            ".knots/events/2026/03/22/0100-knot.acceptance_set.json",
        ))
        .expect("apply should succeed");

    let knot = db::get_knot_hot(&conn, "K-1")
        .expect("query should succeed")
        .expect("knot should exist");
    assert_eq!(knot.acceptance.as_deref(), Some("Synced criteria"));

    let _ = std::fs::remove_dir_all(root);
}
