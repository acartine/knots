use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::db::{self, UpsertKnotHot};
use crate::events::WorkflowPrecondition;
use crate::sync::{GitAdapter, SyncError};

use super::{
    invalid_event, is_stale_precondition, optional_i64, optional_string, parse_metadata_entry,
    read_json_file, IncrementalApplier,
};

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-sync-apply-ext-{}", Uuid::now_v7()));
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
            priority: None,
            knot_type: None,
            tags: &[],
            notes: &[],
            handoff_capsules: &[],
            profile_id: "automation_granular",
            profile_etag: Some("etag-1"),
            deferred_from_state: None,
            created_at: Some("2026-02-25T10:00:00Z"),
        },
    )
    .expect("hot knot should upsert");
}

#[test]
fn helper_functions_cover_optional_and_error_paths() {
    assert_eq!(
        optional_string(Some(&json!("  hello  "))),
        Some("hello".to_string())
    );
    assert_eq!(optional_string(Some(&json!("   "))), None);
    assert_eq!(optional_string(Some(&json!(null))), None);

    assert_eq!(optional_i64(Some(&json!(7))), Some(7));
    assert_eq!(optional_i64(Some(&json!("7"))), None);

    let err = invalid_event(Path::new("/tmp/event.json"), "bad payload");
    assert!(matches!(err, SyncError::InvalidEvent { .. }));
}

#[test]
fn parse_metadata_entry_requires_all_string_fields() {
    let mut valid = Map::<String, Value>::new();
    valid.insert("entry_id".to_string(), json!("n1"));
    valid.insert("content".to_string(), json!("note"));
    valid.insert("username".to_string(), json!("u"));
    valid.insert("datetime".to_string(), json!("2026-02-25T10:00:00Z"));
    valid.insert("agentname".to_string(), json!("codex"));
    valid.insert("model".to_string(), json!("gpt-5"));
    valid.insert("version".to_string(), json!("1"));

    let parsed =
        parse_metadata_entry(&valid, Path::new("/tmp/entry.json")).expect("metadata should parse");
    assert_eq!(parsed.entry_id, "n1");

    let mut missing = valid.clone();
    missing.remove("agentname");
    let err = parse_metadata_entry(&missing, Path::new("/tmp/entry.json"))
        .expect_err("missing field should fail");
    assert!(matches!(err, SyncError::InvalidEvent { .. }));
}

#[test]
fn read_json_file_reports_invalid_payloads() {
    let root = unique_workspace();
    let path = root.join("bad.json");
    std::fs::write(&path, "{").expect("fixture should write");

    let err = read_json_file::<serde_json::Value>(&path).expect_err("invalid JSON should fail");
    assert!(matches!(err, SyncError::InvalidEvent { .. }));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn precondition_checks_cover_none_match_and_mismatch() {
    let root = unique_workspace();
    let conn = open_conn(&root);
    seed_hot_knot(&conn, "K-1");

    let none = is_stale_precondition(&conn, "K-1", None)
        .expect("precondition check without precondition should succeed");
    assert!(!none);

    let matching = is_stale_precondition(
        &conn,
        "K-1",
        Some(&WorkflowPrecondition {
            profile_etag: "etag-1".to_string(),
        }),
    )
    .expect("matching precondition should succeed");
    assert!(!matching);

    let stale = is_stale_precondition(
        &conn,
        "K-1",
        Some(&WorkflowPrecondition {
            profile_etag: "etag-2".to_string(),
        }),
    )
    .expect("stale precondition check should succeed");
    assert!(stale);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn apply_index_event_ignores_missing_and_non_head_files() {
    let root = setup_repo();
    let conn = open_conn(&root);
    let git = GitAdapter::new();
    let applier = IncrementalApplier::new(&conn, root.clone(), git);

    let missing = applier
        .apply_index_event(Path::new(".knots/index/2026/02/25/missing.json"))
        .expect("missing index event should be ignored");
    assert!(!missing);

    let non_head_path = root.join(".knots/index/2026/02/25/1000-idx.other.json");
    std::fs::create_dir_all(
        non_head_path
            .parent()
            .expect("index parent directory should exist"),
    )
    .expect("index parent should be creatable");
    std::fs::write(
        &non_head_path,
        concat!(
            "{\n",
            "  \"event_id\": \"1000\",\n",
            "  \"occurred_at\": \"2026-02-25T10:00:00Z\",\n",
            "  \"type\": \"idx.other\",\n",
            "  \"data\": {\"knot_id\": \"K-1\"}\n",
            "}\n"
        ),
    )
    .expect("index event should write");

    let ignored = applier
        .apply_index_event(Path::new(".knots/index/2026/02/25/1000-idx.other.json"))
        .expect("non-head event should be ignored");
    assert!(!ignored);

    let invalid_data_path = root.join(".knots/index/2026/02/25/1001-idx.knot_head.json");
    std::fs::write(
        &invalid_data_path,
        concat!(
            "{\n",
            "  \"event_id\": \"1001\",\n",
            "  \"occurred_at\": \"2026-02-25T10:00:00Z\",\n",
            "  \"type\": \"idx.knot_head\",\n",
            "  \"data\": \"bad\"\n",
            "}\n"
        ),
    )
    .expect("invalid index event should write");

    let err = applier
        .apply_index_event(Path::new(".knots/index/2026/02/25/1001-idx.knot_head.json"))
        .expect_err("invalid index data should fail");
    assert!(matches!(err, SyncError::InvalidEvent { .. }));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn apply_to_head_reports_snapshot_load_errors_during_bootstrap() {
    let root = setup_repo();
    let conn = open_conn(&root);
    let snapshots = root.join(".knots/snapshots");
    std::fs::create_dir_all(&snapshots).expect("snapshot directory should be creatable");
    std::fs::write(
        snapshots.join("20260225T100000Z-active_catalog.snapshot.json"),
        "{",
    )
    .expect("invalid snapshot fixture should write");

    let mut applier = IncrementalApplier::new(&conn, root.clone(), GitAdapter::new());
    let err = applier
        .apply_to_head("HEAD")
        .expect_err("invalid bootstrap snapshot should fail");
    assert!(matches!(err, SyncError::SnapshotLoad { .. }));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn changed_files_falls_back_to_scan_when_base_revision_is_unknown() {
    let root = setup_repo();
    let conn = open_conn(&root);

    let idx_path = root.join(".knots/index/2026/02/25/3000-idx.knot_head.json");
    std::fs::create_dir_all(
        idx_path
            .parent()
            .expect("index parent directory should exist"),
    )
    .expect("index parent should be creatable");
    std::fs::write(
        &idx_path,
        concat!(
            "{\n",
            "  \"event_id\": \"3000\",\n",
            "  \"occurred_at\": \"2026-02-25T10:00:00Z\",\n",
            "  \"type\": \"idx.knot_head\",\n",
            "  \"data\": {\n",
            "    \"knot_id\": \"K-1\",\n",
            "    \"title\": \"One\",\n",
            "    \"state\": \"work_item\",\n",
            "    \"profile_id\": \"automation_granular\",\n",
            "    \"updated_at\": \"2026-02-25T10:00:00Z\",\n",
            "    \"terminal\": false\n",
            "  }\n",
            "}\n"
        ),
    )
    .expect("index event should write");

    run_git(&root, &["add", ".knots"]);
    run_git(&root, &["commit", "-m", "seed knots index"]);

    let head_output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git rev-parse should run");
    assert!(head_output.status.success());
    let head = String::from_utf8_lossy(&head_output.stdout)
        .trim()
        .to_string();
    db::set_meta(&conn, "last_index_head_commit", &head).expect("meta should set");
    db::set_meta(&conn, "last_full_head_commit", &head).expect("meta should set");

    let mut applier = IncrementalApplier::new(&conn, root.clone(), GitAdapter::new());
    let summary = applier
        .apply_to_head("missing_target_revision")
        .expect("unknown base revision should fall back to scanning files");
    assert_eq!(summary.index_files, 1);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn apply_index_event_moves_old_non_terminal_knots_to_warm_cache() {
    let root = setup_repo();
    let conn = open_conn(&root);
    let applier = IncrementalApplier::new(&conn, root.clone(), GitAdapter::new());

    let idx_path = root.join(".knots/index/2026/02/25/4000-idx.knot_head.json");
    std::fs::create_dir_all(
        idx_path
            .parent()
            .expect("index parent directory should exist"),
    )
    .expect("index parent should be creatable");
    std::fs::write(
        &idx_path,
        concat!(
            "{\n",
            "  \"event_id\": \"4000\",\n",
            "  \"occurred_at\": \"2026-02-25T10:00:00Z\",\n",
            "  \"type\": \"idx.knot_head\",\n",
            "  \"data\": {\n",
            "    \"knot_id\": \"K-warm\",\n",
            "    \"title\": \"Warm candidate\",\n",
            "    \"state\": \"work_item\",\n",
            "    \"profile_id\": \"automation_granular\",\n",
            "    \"updated_at\": \"2020-01-01T00:00:00Z\",\n",
            "    \"terminal\": false\n",
            "  }\n",
            "}\n"
        ),
    )
    .expect("index event should write");
    std::fs::write(root.join(".knots/index/ignore.txt"), "noop")
        .expect("non-json file should write");

    let updated = applier
        .apply_index_event(Path::new(".knots/index/2026/02/25/4000-idx.knot_head.json"))
        .expect("index apply should succeed");
    assert!(updated);
    let hot = db::get_knot_hot(&conn, "K-warm").expect("hot lookup should succeed");
    assert!(hot.is_none());
    let warm = db::get_knot_warm(&conn, "K-warm").expect("warm lookup should succeed");
    assert!(warm.is_some());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn apply_full_event_covers_priority_type_tag_remove_note_and_handoff_paths() {
    let root = setup_repo();
    let conn = open_conn(&root);
    seed_hot_knot(&conn, "K-1");
    let applier = IncrementalApplier::new(&conn, root.clone(), GitAdapter::new());

    let events_dir = root.join(".knots/events/2026/02/25");
    std::fs::create_dir_all(&events_dir).expect("events directory should be creatable");

    let priority_path = events_dir.join("5000-knot.priority_set.json");
    std::fs::write(
        &priority_path,
        concat!(
            "{\n",
            "  \"event_id\": \"5000\",\n",
            "  \"occurred_at\": \"2026-02-25T10:00:00Z\",\n",
            "  \"knot_id\": \"K-1\",\n",
            "  \"type\": \"knot.priority_set\",\n",
            "  \"data\": {\"priority\": 2}\n",
            "}\n"
        ),
    )
    .expect("priority event should write");
    applier
        .apply_full_event(Path::new(
            ".knots/events/2026/02/25/5000-knot.priority_set.json",
        ))
        .expect("priority event should apply");

    let type_path = events_dir.join("5001-knot.type_set.json");
    std::fs::write(
        &type_path,
        concat!(
            "{\n",
            "  \"event_id\": \"5001\",\n",
            "  \"occurred_at\": \"2026-02-25T10:00:00Z\",\n",
            "  \"knot_id\": \"K-1\",\n",
            "  \"type\": \"knot.type_set\",\n",
            "  \"data\": {\"type\": \"task\"}\n",
            "}\n"
        ),
    )
    .expect("type event should write");
    applier
        .apply_full_event(Path::new(
            ".knots/events/2026/02/25/5001-knot.type_set.json",
        ))
        .expect("type event should apply");

    let tag_remove_path = events_dir.join("5002-knot.tag_remove.json");
    std::fs::write(
        &tag_remove_path,
        concat!(
            "{\n",
            "  \"event_id\": \"5002\",\n",
            "  \"occurred_at\": \"2026-02-25T10:00:00Z\",\n",
            "  \"knot_id\": \"K-1\",\n",
            "  \"type\": \"knot.tag_remove\",\n",
            "  \"data\": {\"tag\": \"alpha\"}\n",
            "}\n"
        ),
    )
    .expect("tag remove event should write");
    applier
        .apply_full_event(Path::new(
            ".knots/events/2026/02/25/5002-knot.tag_remove.json",
        ))
        .expect("tag remove event should apply");

    let note_path = events_dir.join("5003-knot.note_added.json");
    std::fs::write(
        &note_path,
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
    )
    .expect("note event should write");
    applier
        .apply_full_event(Path::new(
            ".knots/events/2026/02/25/5003-knot.note_added.json",
        ))
        .expect("note event should apply");

    let handoff_path = events_dir.join("5004-knot.handoff_capsule_added.json");
    std::fs::write(
        &handoff_path,
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
    )
    .expect("handoff event should write");
    applier
        .apply_full_event(Path::new(
            ".knots/events/2026/02/25/5004-knot.handoff_capsule_added.json",
        ))
        .expect("handoff event should apply");

    let note_missing_hot = events_dir.join("5005-knot.note_added.json");
    std::fs::write(
        &note_missing_hot,
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
    )
    .expect("missing-hot note event should write");
    applier
        .apply_full_event(Path::new(
            ".knots/events/2026/02/25/5005-knot.note_added.json",
        ))
        .expect("missing-hot note event should still apply as ignored");

    let updated = db::get_knot_hot(&conn, "K-1")
        .expect("hot lookup should succeed")
        .expect("hot knot should remain present");
    assert_eq!(updated.priority, Some(2));
    assert_eq!(updated.knot_type.as_deref(), Some("task"));
    assert_eq!(updated.notes.len(), 1);
    assert_eq!(updated.handoff_capsules.len(), 1);

    let _ = std::fs::remove_dir_all(root);
}
