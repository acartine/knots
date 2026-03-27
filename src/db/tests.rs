use super::{
    get_pull_drift_warn_threshold, get_sync_fetch_blob_limit_kb, open_connection, set_meta,
    CURRENT_SCHEMA_VERSION,
};
use rusqlite::params;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
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

    let drift_warn_threshold: String = conn
        .query_row(
            "SELECT value FROM meta WHERE key='pull_drift_warn_threshold'",
            [],
            |row| row.get(0),
        )
        .expect("pull_drift_warn_threshold default should exist");
    assert_eq!(drift_warn_threshold, "25");

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
fn migrations_add_parity_columns_and_backfill_profile_defaults() {
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
VALUES (2, 'reserved_v2', '2026-02-23T00:00:01Z');

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

CREATE TABLE cold_catalog (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    state TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
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
        "invariants_json",
        "profile_id",
        "profile_etag",
        "deferred_from_state",
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

    let profile_id: String = upgraded
        .query_row(
            "SELECT profile_id FROM knot_hot WHERE id = 'K-legacy'",
            [],
            |row| row.get(0),
        )
        .expect("legacy row should include profile_id");
    assert_eq!(profile_id, "autopilot");

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

#[test]
fn reads_pull_drift_warn_threshold_from_meta() {
    let path = unique_db_path();
    let conn = open_connection(&path).expect("connection should open");

    let initial =
        get_pull_drift_warn_threshold(&conn).expect("drift warning threshold should read");
    assert_eq!(initial, 25);

    set_meta(&conn, "pull_drift_warn_threshold", "5").expect("meta update should succeed");
    let configured =
        get_pull_drift_warn_threshold(&conn).expect("drift warning threshold should read");
    assert_eq!(configured, 5);

    cleanup_db_files(&path);
}

#[test]
fn open_connection_stays_readable_when_writer_lock_is_held() {
    let path = unique_db_path();
    let initialized = open_connection(&path).expect("initial connection should open");
    drop(initialized);

    let lock_conn = rusqlite::Connection::open(&path).expect("lock connection should open");
    lock_conn
        .execute_batch("BEGIN IMMEDIATE;")
        .expect("write lock should be acquirable");

    let (tx, rx) = mpsc::channel();
    let path_clone = path.clone();
    thread::spawn(move || {
        let result = open_connection(&path_clone).map(|_| ());
        tx.send(result)
            .expect("lock probe channel should accept one message");
    });

    let result = rx
        .recv_timeout(Duration::from_millis(750))
        .expect("open_connection should not block behind an unrelated writer");
    result.expect("second connection should open while writer lock is held");

    lock_conn
        .execute_batch("ROLLBACK;")
        .expect("write lock should release");
    cleanup_db_files(&path);
}

#[test]
fn set_meta_retries_when_database_is_temporarily_locked() {
    let path = unique_db_path();
    let seeded = open_connection(&path).expect("seed connection should open");
    drop(seeded);

    let lock_conn = rusqlite::Connection::open(&path).expect("lock connection should open");
    lock_conn
        .execute_batch("BEGIN IMMEDIATE;")
        .expect("write lock should be acquirable");
    let unlock_thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(25));
        lock_conn
            .execute_batch("ROLLBACK;")
            .expect("write lock should release");
    });

    let conn = open_connection(&path).expect("test connection should open");
    conn.pragma_update(None::<rusqlite::DatabaseName>, "busy_timeout", 1i64)
        .expect("busy_timeout pragma should update");
    conn.busy_timeout(Duration::from_millis(1))
        .expect("busy timeout API should update");

    set_meta(&conn, "sync_policy", "always")
        .expect("set_meta should retry and succeed after lock release");
    unlock_thread
        .join()
        .expect("unlock thread should complete successfully");

    let value: String = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'sync_policy'",
            [],
            |row| row.get(0),
        )
        .expect("sync_policy row should be readable");
    assert_eq!(value, "always");

    cleanup_db_files(&path);
}

#[test]
fn upsert_and_get_knot_hot_round_trips_invariants() {
    use crate::db::{get_knot_hot, upsert_knot_hot, UpsertKnotHot};
    use crate::domain::invariant::{Invariant, InvariantType};

    let path = unique_db_path();
    let conn = open_connection(&path).expect("connection should open");

    let invariants = vec![
        Invariant::new(InvariantType::Scope, "only touch src/db.rs").unwrap(),
        Invariant::new(InvariantType::State, "coverage >= 95%").unwrap(),
    ];

    upsert_knot_hot(
        &conn,
        &UpsertKnotHot {
            id: "K-inv",
            title: "Invariant round-trip",
            state: "implementation",
            updated_at: "2026-03-05T10:00:00Z",
            body: None,
            description: Some("test invariants"),
            acceptance: None,
            priority: None,
            knot_type: Some("work"),
            tags: &["alpha".to_string()],
            notes: &[],
            handoff_capsules: &[],
            invariants: &invariants,
            step_history: &[],
            gate_data: &crate::domain::gate::GateData::default(),
            lease_data: &crate::domain::lease::LeaseData::default(),
            lease_id: None,
            workflow_id: "compatibility",
            profile_id: "autopilot",
            profile_etag: Some("etag-inv"),
            deferred_from_state: None,
            blocked_from_state: None,
            created_at: Some("2026-03-05T09:00:00Z"),
        },
    )
    .expect("upsert with invariants should succeed");

    let record = get_knot_hot(&conn, "K-inv")
        .expect("get should succeed")
        .expect("record should exist");
    assert_eq!(record.invariants.len(), 2);
    assert_eq!(record.invariants[0].invariant_type, InvariantType::Scope);
    assert_eq!(record.invariants[0].condition, "only touch src/db.rs");
    assert_eq!(record.invariants[1].invariant_type, InvariantType::State);
    assert_eq!(record.invariants[1].condition, "coverage >= 95%");

    cleanup_db_files(&path);
}

#[test]
fn upsert_knot_hot_with_empty_invariants_round_trips() {
    use crate::db::{get_knot_hot, upsert_knot_hot, UpsertKnotHot};

    let path = unique_db_path();
    let conn = open_connection(&path).expect("connection should open");

    upsert_knot_hot(
        &conn,
        &UpsertKnotHot {
            id: "K-no-inv",
            title: "No invariants",
            state: "ready_for_planning",
            updated_at: "2026-03-05T10:00:00Z",
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
            profile_etag: None,
            deferred_from_state: None,
            blocked_from_state: None,
            created_at: None,
        },
    )
    .expect("upsert with empty invariants should succeed");

    let record = get_knot_hot(&conn, "K-no-inv")
        .expect("get should succeed")
        .expect("record should exist");
    assert!(record.invariants.is_empty());

    cleanup_db_files(&path);
}

#[test]
fn count_active_leases_returns_count() {
    use crate::db::{count_active_leases, upsert_knot_hot, UpsertKnotHot};
    use crate::domain::lease::LeaseData;

    let path = unique_db_path();
    let conn = open_connection(&path).expect("connection should open");

    let empty = count_active_leases(&conn).expect("count should succeed on empty db");
    assert_eq!(empty, 0);

    let gate_data = crate::domain::gate::GateData::default();
    for (id, state) in [
        ("K-lease-1", "lease_ready"),
        ("K-lease-2", "lease_active"),
        ("K-lease-3", "lease_terminated"),
        ("K-work-1", "implementation"),
    ] {
        let knot_type = if id.starts_with("K-lease") {
            Some("lease")
        } else {
            Some("work")
        };
        upsert_knot_hot(
            &conn,
            &UpsertKnotHot {
                id,
                title: id,
                state,
                updated_at: "2026-03-12T00:00:00Z",
                body: None,
                description: None,
                acceptance: None,
                priority: None,
                knot_type,
                tags: &[],
                notes: &[],
                handoff_capsules: &[],
                invariants: &[],
                step_history: &[],
                gate_data: &gate_data,
                lease_data: &LeaseData::default(),
                lease_id: None,
                workflow_id: "compatibility",
                profile_id: "autopilot",
                profile_etag: None,
                deferred_from_state: None,
                blocked_from_state: None,
                created_at: None,
            },
        )
        .expect("upsert should succeed");
    }

    let count = count_active_leases(&conn).expect("count should succeed");
    assert_eq!(count, 2);

    cleanup_db_files(&path);
}

#[test]
fn get_knot_hot_accepts_legacy_empty_lease_data_json() {
    use crate::db::{get_knot_hot, upsert_knot_hot, UpsertKnotHot};

    let path = unique_db_path();
    let conn = open_connection(&path).expect("connection should open");

    upsert_knot_hot(
        &conn,
        &UpsertKnotHot {
            id: "K-legacy-lease",
            title: "Legacy lease",
            state: "implementation",
            updated_at: "2026-03-18T10:00:00Z",
            body: None,
            description: None,
            acceptance: None,
            priority: None,
            knot_type: Some("work"),
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
            profile_etag: None,
            deferred_from_state: None,
            blocked_from_state: None,
            created_at: None,
        },
    )
    .expect("upsert should succeed");

    conn.execute(
        "UPDATE knot_hot SET lease_data_json = '{}' WHERE id = ?1",
        params!["K-legacy-lease"],
    )
    .expect("legacy lease payload should update");

    let record = get_knot_hot(&conn, "K-legacy-lease")
        .expect("legacy read should succeed")
        .expect("record should exist");
    assert_eq!(
        record.lease_data,
        crate::domain::lease::LeaseData::default()
    );

    cleanup_db_files(&path);
}
