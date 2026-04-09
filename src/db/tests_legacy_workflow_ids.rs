use crate::db::open_connection;

fn unique_db_path() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before UNIX_EPOCH")
        .as_nanos();
    std::env::temp_dir()
        .join(format!("knots-legacy-workflow-{}.sqlite", nanos))
        .display()
        .to_string()
}

fn cleanup_db_files(path: &str) {
    for suffix in ["", "-wal", "-shm"] {
        let candidate = format!("{path}{suffix}");
        let _ = std::fs::remove_file(candidate);
    }
}

fn column_default(
    conn: &rusqlite::Connection,
    table_name: &str,
    column_name: &str,
) -> Option<String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({})", table_name))
        .expect("table info pragma should prepare");
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, Option<String>>(4)?))
        })
        .expect("table info rows should be readable");
    for item in rows {
        let (name, default_value) = item.expect("column info should read");
        if name == column_name {
            return default_value;
        }
    }
    None
}

const SCHEMA_V15_KNOTS_SDLC_FIXTURE: &str = r#"
CREATE TABLE schema_migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    applied_at TEXT NOT NULL
);
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (1, 'baseline_cache_schema_v1', '2026-02-23T00:00:00Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (2, 'reserved_v2', '2026-02-23T00:00:01Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (3, 'knot_field_parity_v1', '2026-02-23T00:00:02Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (4, 'knot_workflow_identity_v1', '2026-02-23T00:00:03Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (5, 'workflow_id_canonicalize_v1', '2026-02-23T00:00:04Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (6, 'workflow_to_profile_v1', '2026-02-23T00:00:05Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (7, 'knot_invariants_v1', '2026-02-23T00:00:06Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (8, 'knot_step_history_v1', '2026-02-23T00:00:07Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (9, 'knot_gate_data_v1', '2026-02-23T00:00:08Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (10, 'knot_lease_data_v1', '2026-02-23T00:00:09Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (11, 'knot_workflow_id_v2', '2026-02-23T00:00:10Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (12, 'knot_acceptance_v1', '2026-02-23T00:00:11Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (13, 'knot_blocked_provenance_v1', '2026-02-23T00:00:12Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (14, 'lease_expiry_v1', '2026-02-23T00:00:13Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (15, 'builtin_workflow_id_knots_sdlc_v1', '2026-02-23T00:00:14Z');

CREATE TABLE meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
INSERT INTO meta (key, value) VALUES ('schema_version', '15');
INSERT INTO meta (key, value) VALUES ('hot_window_days', '7');
INSERT INTO meta (key, value) VALUES ('sync_policy', 'auto');
INSERT INTO meta (key, value) VALUES ('sync_auto_budget_ms', '750');
INSERT INTO meta (key, value) VALUES ('sync_try_lock_ms', '0');
INSERT INTO meta (key, value) VALUES ('push_retry_budget_ms', '800');
INSERT INTO meta (key, value) VALUES ('sync_fetch_blob_limit_kb', '0');
INSERT INTO meta (key, value) VALUES ('pull_drift_warn_threshold', '25');

CREATE TABLE knot_hot (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    state TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    body TEXT,
    description TEXT,
    priority INTEGER,
    knot_type TEXT,
    tags_json TEXT NOT NULL DEFAULT '[]',
    notes_json TEXT NOT NULL DEFAULT '[]',
    handoff_capsules_json TEXT NOT NULL DEFAULT '[]',
    invariants_json TEXT NOT NULL DEFAULT '[]',
    step_history_json TEXT NOT NULL DEFAULT '[]',
    gate_data_json TEXT NOT NULL DEFAULT '{}',
    lease_data_json TEXT NOT NULL DEFAULT '{}',
    lease_id TEXT,
    workflow_id TEXT NOT NULL DEFAULT 'knots_sdlc',
    profile_id TEXT NOT NULL DEFAULT 'autopilot',
    profile_etag TEXT,
    deferred_from_state TEXT,
    acceptance TEXT,
    blocked_from_state TEXT,
    lease_expiry_ts INTEGER NOT NULL DEFAULT 0,
    created_at TEXT
);
INSERT INTO knot_hot (
    id, title, state, updated_at, workflow_id, profile_id
) VALUES (
    'K-legacy', 'Legacy', 'ready_for_planning', '2026-02-23T00:00:15Z',
    'knots_sdlc', 'autopilot'
);
"#;

#[test]
fn migration_rewrites_knots_sdlc_builtin_workflow_ids_to_work_sdlc() {
    let path = unique_db_path();
    let conn = rusqlite::Connection::open(&path).expect("pre-migration connection should open");
    conn.execute_batch(SCHEMA_V15_KNOTS_SDLC_FIXTURE)
        .expect("schema v15 fixture should be writable");
    drop(conn);

    let upgraded = open_connection(&path).expect("open_connection should apply migration 16");
    let workflow_id: String = upgraded
        .query_row(
            "SELECT workflow_id FROM knot_hot WHERE id = 'K-legacy'",
            [],
            |row| row.get(0),
        )
        .expect("legacy row should include repaired workflow_id");
    assert_eq!(workflow_id, "work_sdlc");
    assert_eq!(
        column_default(&upgraded, "knot_hot", "workflow_id").as_deref(),
        Some("'work_sdlc'")
    );

    cleanup_db_files(&path);
}
