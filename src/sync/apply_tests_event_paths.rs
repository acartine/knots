use std::path::{Path, PathBuf};
use std::process::Command;

use uuid::Uuid;

use crate::db::{self, UpsertKnotHot};
use crate::sync::{GitAdapter, SyncError};

use super::{read_json_file, IncrementalApplier};

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir()
        .join(format!("knots-sync-apply-evpaths-{}", Uuid::now_v7()));
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
    std::fs::write(root.join("README.md"), "# apply\n")
        .expect("readme should be writable");
    run_git(&root, &["add", "README.md"]);
    run_git(&root, &["commit", "-m", "init"]);
    root
}

fn open_conn(root: &Path) -> rusqlite::Connection {
    let db_path = root.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(
        db_path.parent().expect("db parent should exist"),
    )
    .expect("db parent should be creatable");
    db::open_connection(db_path.to_str().expect("utf8 db path"))
        .expect("db should open")
}

fn seed_hot_knot(conn: &rusqlite::Connection, knot_id: &str) {
    db::upsert_knot_hot(
        conn,
        &UpsertKnotHot {
            id: knot_id,
            title: "Seed",
            state: "work_item",
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
            profile_id: "automation_granular",
            profile_etag: Some("etag-1"),
            deferred_from_state: None,
            blocked_from_state: None,
            created_at: Some("2026-02-25T10:00:00Z"),
        },
    )
    .expect("hot knot should upsert");
}

#[test]
fn read_json_file_reports_invalid_payloads() {
    let root = unique_workspace();
    let path = root.join("bad.json");
    std::fs::write(&path, "{").expect("fixture should write");

    let err = read_json_file::<serde_json::Value>(&path)
        .expect_err("invalid JSON should fail");
    assert!(matches!(err, SyncError::InvalidEvent { .. }));

    let _ = std::fs::remove_dir_all(root);
}

fn write_event_file(
    events_dir: &Path,
    filename: &str,
    content: &str,
) {
    let path = events_dir.join(filename);
    std::fs::write(&path, content).expect("event should write");
}

fn apply_priority_and_type_events(
    applier: &IncrementalApplier<'_>,
    events_dir: &Path,
) {
    write_event_file(
        events_dir,
        "5000-knot.priority_set.json",
        concat!(
            "{\n",
            "  \"event_id\": \"5000\",\n",
            "  \"occurred_at\": \"2026-02-25T10:00:00Z\",\n",
            "  \"knot_id\": \"K-1\",\n",
            "  \"type\": \"knot.priority_set\",\n",
            "  \"data\": {\"priority\": 2}\n",
            "}\n"
        ),
    );
    applier
        .apply_full_event(Path::new(
            ".knots/events/2026/02/25/5000-knot.priority_set.json",
        ))
        .expect("priority event should apply");

    write_event_file(
        events_dir,
        "5001-knot.type_set.json",
        concat!(
            "{\n",
            "  \"event_id\": \"5001\",\n",
            "  \"occurred_at\": \"2026-02-25T10:00:00Z\",\n",
            "  \"knot_id\": \"K-1\",\n",
            "  \"type\": \"knot.type_set\",\n",
            "  \"data\": {\"type\": \"task\"}\n",
            "}\n"
        ),
    );
    applier
        .apply_full_event(Path::new(
            ".knots/events/2026/02/25/5001-knot.type_set.json",
        ))
        .expect("type event should apply");
}

fn apply_tag_note_handoff_events(
    applier: &IncrementalApplier<'_>,
    events_dir: &Path,
) {
    write_event_file(
        events_dir,
        "5002-knot.tag_remove.json",
        concat!(
            "{\n",
            "  \"event_id\": \"5002\",\n",
            "  \"occurred_at\": \"2026-02-25T10:00:00Z\",\n",
            "  \"knot_id\": \"K-1\",\n",
            "  \"type\": \"knot.tag_remove\",\n",
            "  \"data\": {\"tag\": \"alpha\"}\n",
            "}\n"
        ),
    );
    applier
        .apply_full_event(Path::new(
            ".knots/events/2026/02/25/5002-knot.tag_remove.json",
        ))
        .expect("tag remove event should apply");

    write_event_file(
        events_dir,
        "5003-knot.note_added.json",
        concat!(
            "{\n",
            "  \"event_id\": \"5003\",\n",
            "  \"occurred_at\": \"2026-02-25T10:00:00Z\",\n",
            "  \"knot_id\": \"K-1\",\n",
            "  \"type\": \"knot.note_added\",\n",
            "  \"data\": {\n",
            "    \"entry_id\": \"n1\",\n",
            "    \"content\": \"note\",\n",
            "    \"username\": \"u\",\n",
            "    \"datetime\": \"2026-02-25T10:00:00Z\",\n",
            "    \"agentname\": \"codex\",\n",
            "    \"model\": \"gpt-5\",\n",
            "    \"version\": \"1\"\n",
            "  }\n",
            "}\n"
        ),
    );
    applier
        .apply_full_event(Path::new(
            ".knots/events/2026/02/25/5003-knot.note_added.json",
        ))
        .expect("note event should apply");

    write_event_file(
        events_dir,
        "5004-knot.handoff_capsule_added.json",
        concat!(
            "{\n",
            "  \"event_id\": \"5004\",\n",
            "  \"occurred_at\": \"2026-02-25T10:00:00Z\",\n",
            "  \"knot_id\": \"K-1\",\n",
            "  \"type\": \"knot.handoff_capsule_added\",\n",
            "  \"data\": {\n",
            "    \"entry_id\": \"h1\",\n",
            "    \"content\": \"handoff\",\n",
            "    \"username\": \"u\",\n",
            "    \"datetime\": \"2026-02-25T10:00:00Z\",\n",
            "    \"agentname\": \"codex\",\n",
            "    \"model\": \"gpt-5\",\n",
            "    \"version\": \"1\"\n",
            "  }\n",
            "}\n"
        ),
    );
    applier
        .apply_full_event(Path::new(
            ".knots/events/2026/02/25/5004-knot.handoff_capsule_added.json",
        ))
        .expect("handoff event should apply");
}

fn apply_missing_hot_note_event(
    applier: &IncrementalApplier<'_>,
    events_dir: &Path,
) {
    write_event_file(
        events_dir,
        "5005-knot.note_added.json",
        concat!(
            "{\n",
            "  \"event_id\": \"5005\",\n",
            "  \"occurred_at\": \"2026-02-25T10:00:00Z\",\n",
            "  \"knot_id\": \"K-missing\",\n",
            "  \"type\": \"knot.note_added\",\n",
            "  \"data\": {\n",
            "    \"entry_id\": \"n2\",\n",
            "    \"content\": \"note\",\n",
            "    \"username\": \"u\",\n",
            "    \"datetime\": \"2026-02-25T10:00:00Z\",\n",
            "    \"agentname\": \"codex\",\n",
            "    \"model\": \"gpt-5\",\n",
            "    \"version\": \"1\"\n",
            "  }\n",
            "}\n"
        ),
    );
    applier
        .apply_full_event(Path::new(
            ".knots/events/2026/02/25/5005-knot.note_added.json",
        ))
        .expect("missing-hot note event should still apply as ignored");
}

#[test]
fn apply_full_event_covers_priority_type_tag_remove_note_and_handoff() {
    let root = setup_repo();
    let conn = open_conn(&root);
    seed_hot_knot(&conn, "K-1");
    let applier =
        IncrementalApplier::new(&conn, root.clone(), GitAdapter::new());

    let events_dir = root.join(".knots/events/2026/02/25");
    std::fs::create_dir_all(&events_dir)
        .expect("events directory should be creatable");

    apply_priority_and_type_events(&applier, &events_dir);
    apply_tag_note_handoff_events(&applier, &events_dir);
    apply_missing_hot_note_event(&applier, &events_dir);

    let updated = db::get_knot_hot(&conn, "K-1")
        .expect("hot lookup should succeed")
        .expect("hot knot should remain present");
    assert_eq!(updated.priority, Some(2));
    assert_eq!(updated.knot_type.as_deref(), Some("task"));
    assert_eq!(updated.notes.len(), 1);
    assert_eq!(updated.handoff_capsules.len(), 1);

    let _ = std::fs::remove_dir_all(root);
}
