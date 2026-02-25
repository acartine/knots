use super::{get_sync_fetch_blob_limit_kb, open_connection, set_meta, CURRENT_SCHEMA_VERSION};
use rusqlite::params;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db_path() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX_EPOCH")
        .as_nanos();
    std::env::temp_dir()
        .join(format!("knots-pragmas-{}.sqlite", nanos))
        .display()
        .to_string()
}

fn cleanup_db_files(path: &str) {
    for suffix in ["", "-wal", "-shm"] {
        let candidate = format!("{path}{suffix}");
        let _ = std::fs::remove_file(candidate);
    }
}

fn table_exists(conn: &rusqlite::Connection, table_name: &str) -> bool {
    let exists: i64 = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            params![table_name],
            |row| row.get(0),
        )
        .expect("table existence query should be readable");
    exists == 1
}

fn column_exists(conn: &rusqlite::Connection, table_name: &str, column_name: &str) -> bool {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({})", table_name))
        .expect("table info pragma should prepare");
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .expect("table info rows should be readable");
    for item in rows {
        if item.expect("column name should read") == column_name {
            return true;
        }
    }
    false
}

#[test]
fn configures_connection_pragmas() {
    let path = unique_db_path();
    let conn = open_connection(&path).expect("connection should open");

    let journal_mode: String = conn
        .query_row("PRAGMA journal_mode;", [], |row| row.get(0))
        .expect("journal_mode pragma should be readable");
    assert_eq!(journal_mode.to_uppercase(), "WAL");

    let synchronous: i64 = conn
        .query_row("PRAGMA synchronous;", [], |row| row.get(0))
        .expect("synchronous pragma should be readable");
    assert_eq!(synchronous, 1);

    let foreign_keys: i64 = conn
        .query_row("PRAGMA foreign_keys;", [], |row| row.get(0))
        .expect("foreign_keys pragma should be readable");
    assert_eq!(foreign_keys, 1);

    let temp_store: i64 = conn
        .query_row("PRAGMA temp_store;", [], |row| row.get(0))
        .expect("temp_store pragma should be readable");
    assert_eq!(temp_store, 2);

    let busy_timeout: i64 = conn
        .query_row("PRAGMA busy_timeout;", [], |row| row.get(0))
        .expect("busy_timeout pragma should be readable");
    assert_eq!(busy_timeout, 5000);

    cleanup_db_files(&path);
}

#[test]
fn initializes_required_tables_and_schema_version() {
    let path = unique_db_path();
    let conn = open_connection(&path).expect("connection should open");

    let tables = [
        "schema_migrations",
        "meta",
        "knot_hot",
        "knot_warm",
        "edge",
        "review_stats",
        "cold_catalog",
        "import_state",
        "import_fingerprints",
    ];
    for table in tables {
        assert!(
            table_exists(&conn, table),
            "expected table '{}' to exist",
            table
        );
    }

    let schema_version: String = conn
        .query_row(
            "SELECT value FROM meta WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .expect("schema version should be stored in meta table");
    assert_eq!(schema_version, CURRENT_SCHEMA_VERSION.to_string());

    let hot_window_days: String = conn
        .query_row(
            "SELECT value FROM meta WHERE key='hot_window_days'",
            [],
            |row| row.get(0),
        )
        .expect("hot_window_days default should exist");
    assert_eq!(hot_window_days, "7");

    let sync_policy: String = conn
        .query_row(
            "SELECT value FROM meta WHERE key='sync_policy'",
            [],
            |row| row.get(0),
        )
        .expect("sync_policy default should exist");
    assert_eq!(sync_policy, "auto");

    let push_retry_budget_ms: String = conn
        .query_row(
            "SELECT value FROM meta WHERE key='push_retry_budget_ms'",
            [],
            |row| row.get(0),
        )
        .expect("push_retry_budget_ms default should exist");
    assert_eq!(push_retry_budget_ms, "800");

    let fetch_blob_limit: String = conn
        .query_row(
            "SELECT value FROM meta WHERE key='sync_fetch_blob_limit_kb'",
            [],
            |row| row.get(0),
        )
        .expect("sync_fetch_blob_limit_kb default should exist");
    assert_eq!(fetch_blob_limit, "0");

    cleanup_db_files(&path);
}

#[test]
fn reapplies_migrations_idempotently() {
    let path = unique_db_path();
    let conn_first = open_connection(&path).expect("first open should initialize schema");
    drop(conn_first);

    let conn_second = open_connection(&path).expect("second open should be idempotent");
    let applied_count: i64 = conn_second
        .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .expect("schema_migrations count should be queryable");
    assert_eq!(applied_count, CURRENT_SCHEMA_VERSION);

    let schema_version: String = conn_second
        .query_row(
            "SELECT value FROM meta WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .expect("schema version should be queryable");
    assert_eq!(schema_version, CURRENT_SCHEMA_VERSION.to_string());

    cleanup_db_files(&path);
}

#[test]
fn migrations_add_parity_columns_and_backfill_workflow_defaults() {
    let path = unique_db_path();
    let conn = rusqlite::Connection::open(&path).expect("pre-migration connection should open");
    conn.execute_batch(
        r#"
CREATE TABLE schema_migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    applied_at TEXT NOT NULL
);
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (1, 'baseline_cache_schema_v1', '2026-02-23T00:00:00Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (2, 'import_tracking_v1', '2026-02-23T00:00:01Z');

CREATE TABLE meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
INSERT INTO meta (key, value) VALUES ('schema_version', '2');
INSERT INTO meta (key, value) VALUES ('hot_window_days', '7');

CREATE TABLE knot_hot (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    state TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    body TEXT,
    workflow_etag TEXT,
    created_at TEXT,
    metadata_json TEXT
);
INSERT INTO knot_hot (id, title, state, updated_at, body, workflow_etag, created_at)
VALUES ('K-legacy', 'Legacy', 'work_item', '2026-02-23T00:00:02Z', 'legacy body', NULL, NULL);
"#,
    )
    .expect("pre-migration schema should be writable");
    drop(conn);

    let upgraded = open_connection(&path).expect("open_connection should apply migrations");
    for column in [
        "description",
        "priority",
        "knot_type",
        "tags_json",
        "notes_json",
        "handoff_capsules_json",
        "workflow_id",
    ] {
        assert!(
            column_exists(&upgraded, "knot_hot", column),
            "expected knot_hot.{} column after migration",
            column
        );
    }

    let description: Option<String> = upgraded
        .query_row(
            "SELECT description FROM knot_hot WHERE id = 'K-legacy'",
            [],
            |row| row.get(0),
        )
        .expect("legacy row should be queryable");
    assert_eq!(description.as_deref(), Some("legacy body"));

    let workflow_id: String = upgraded
        .query_row(
            "SELECT workflow_id FROM knot_hot WHERE id = 'K-legacy'",
            [],
            |row| row.get(0),
        )
        .expect("legacy row should include workflow_id");
    assert_eq!(workflow_id, "default");

    cleanup_db_files(&path);
}

#[test]
fn reads_optional_fetch_blob_limit_from_meta() {
    let path = unique_db_path();
    let conn = open_connection(&path).expect("connection should open");

    let initial = get_sync_fetch_blob_limit_kb(&conn).expect("fetch blob limit should read");
    assert_eq!(initial, None);

    set_meta(&conn, "sync_fetch_blob_limit_kb", "4").expect("meta update should succeed");
    let configured = get_sync_fetch_blob_limit_kb(&conn).expect("fetch blob limit should read");
    assert_eq!(configured, Some(4));

    cleanup_db_files(&path);
}
