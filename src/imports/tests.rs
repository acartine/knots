use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::db;
use crate::events::EventWriter;

use super::service::ImportService;
use super::source::{source_key, SourceKind};

fn unique_workspace() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX_EPOCH")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("knots-import-test-{}", nanos));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    root
}

#[test]
fn jsonl_import_writes_events_and_cache() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(db_path.parent().expect("db path parent should exist"))
        .expect("db parent directory should be creatable");

    let conn = db::open_connection(db_path.to_str().expect("utf8 path")).expect("db should open");
    let writer = EventWriter::new(&root);
    let service = ImportService::new(&conn, &writer);

    let input = root.join("issues.jsonl");
    std::fs::write(
        &input,
        concat!(
            "{\"id\":\"X-1\",\"title\":\"First\",\"description\":\"a\",\"status\":\"open\",",
            "\"created_at\":\"2026-02-20T10:00:00Z\",",
            "\"updated_at\":\"2026-02-20T10:00:00Z\"}\n",
            "{\"id\":\"X-2\",\"title\":\"Second\",\"status\":\"closed\",\"labels\":[\"a\"],",
            "\"created_at\":\"2026-02-20T11:00:00Z\",",
            "\"updated_at\":\"2026-02-20T12:00:00Z\"}\n"
        ),
    )
    .expect("jsonl should be writable");

    let summary = service
        .import_jsonl(input.to_str().expect("utf8 path"), None, false)
        .expect("import should succeed");
    assert_eq!(summary.imported_count, 2);
    assert_eq!(summary.error_count, 0);
    assert_eq!(summary.status, "completed");

    let knots = db::list_knot_hot(&conn).expect("cache list should succeed");
    assert_eq!(knots.len(), 2);

    let status = service.list_statuses().expect("status should load");
    assert_eq!(status.len(), 1);
    assert_eq!(status[0].processed_count, 2);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn jsonl_resume_uses_checkpoint_and_idempotency() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(db_path.parent().expect("db path parent should exist"))
        .expect("db parent directory should be creatable");

    let conn = db::open_connection(db_path.to_str().expect("utf8 path")).expect("db should open");
    let writer = EventWriter::new(&root);
    let service = ImportService::new(&conn, &writer);

    let input = root.join("issues.jsonl");
    std::fs::write(
        &input,
        concat!(
            "{\"id\":\"R-1\",\"title\":\"First\",\"status\":\"open\",",
            "\"updated_at\":\"2026-02-21T10:00:00Z\"}\n"
        ),
    )
    .expect("jsonl should be writable");
    let first = service
        .import_jsonl(input.to_str().expect("utf8 path"), None, false)
        .expect("first import should succeed");
    assert_eq!(first.imported_count, 1);
    assert_eq!(first.checkpoint.as_deref(), Some("1"));

    std::fs::write(
        &input,
        concat!(
            "{\"id\":\"R-1\",\"title\":\"First\",\"status\":\"open\",",
            "\"updated_at\":\"2026-02-21T10:00:00Z\"}\n",
            "{\"id\":\"R-2\",\"title\":\"Second\",\"status\":\"open\",",
            "\"updated_at\":\"2026-02-21T11:00:00Z\"}\n"
        ),
    )
    .expect("jsonl should be writable");
    let resumed = service
        .import_jsonl(input.to_str().expect("utf8 path"), None, false)
        .expect("resumed import should succeed");
    assert_eq!(resumed.processed_count, 1);
    assert_eq!(resumed.imported_count, 1);
    assert_eq!(resumed.checkpoint.as_deref(), Some("2"));

    let source = std::fs::canonicalize(&input).expect("canonical path should exist");
    let key = source_key(SourceKind::Jsonl, &source.to_string_lossy());
    conn.execute(
        "UPDATE import_state SET checkpoint = NULL WHERE source_key = ?1",
        [key],
    )
    .expect("checkpoint reset should work");

    let replay = service
        .import_jsonl(input.to_str().expect("utf8 path"), None, false)
        .expect("replay import should succeed");
    assert_eq!(replay.imported_count, 0);
    assert_eq!(replay.skipped_count, 2);

    let _ = std::fs::remove_dir_all(root);
}
