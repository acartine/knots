use super::{open_connection, CURRENT_SCHEMA_VERSION};
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
