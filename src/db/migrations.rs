use rusqlite::{params, Connection, OptionalExtension, Result};

use super::{get_meta, now_utc_rfc3339, CURRENT_SCHEMA_VERSION};

const REQUIRED_META_DEFAULT_KEYS: [&str; 7] = [
    "hot_window_days",
    "sync_policy",
    "sync_auto_budget_ms",
    "sync_try_lock_ms",
    "push_retry_budget_ms",
    "sync_fetch_blob_limit_kb",
    "pull_drift_warn_threshold",
];

struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}


const MIGRATIONS: [Migration; 13] = [
    Migration {
        version: 1,
        name: "baseline_cache_schema_v1",
        sql: r#"
CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS knot_hot (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    state TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    body TEXT,
    workflow_etag TEXT,
    created_at TEXT,
    metadata_json TEXT
);

CREATE TABLE IF NOT EXISTS knot_warm (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS edge (
    src TEXT NOT NULL,
    kind TEXT NOT NULL,
    dst TEXT NOT NULL,
    PRIMARY KEY (src, kind, dst)
);

CREATE TABLE IF NOT EXISTS review_stats (
    id TEXT PRIMARY KEY,
    rework_count INTEGER NOT NULL DEFAULT 0,
    last_decision_at TEXT,
    last_outcome TEXT
);

CREATE TABLE IF NOT EXISTS cold_catalog (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    state TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_knot_hot_updated_at ON knot_hot(updated_at);
CREATE INDEX IF NOT EXISTS idx_knot_hot_state ON knot_hot(state);
CREATE INDEX IF NOT EXISTS idx_edge_dst_kind ON edge(dst, kind);
CREATE INDEX IF NOT EXISTS idx_cold_catalog_updated_at ON cold_catalog(updated_at);
"#,
    },
    Migration {
        version: 2,
        name: "reserved_v2",
        sql: r#"
-- Reserved for backward compatibility with previously shipped schema version 2.
"#,
    },
    Migration {
        version: 3,
        name: "knot_field_parity_v1",
        sql: r#"
ALTER TABLE knot_hot ADD COLUMN description TEXT;
ALTER TABLE knot_hot ADD COLUMN priority INTEGER;
ALTER TABLE knot_hot ADD COLUMN knot_type TEXT;
ALTER TABLE knot_hot ADD COLUMN tags_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE knot_hot ADD COLUMN notes_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE knot_hot ADD COLUMN handoff_capsules_json TEXT NOT NULL DEFAULT '[]';

UPDATE knot_hot
SET description = COALESCE(description, body)
WHERE description IS NULL;
"#,
    },
    Migration {
        version: 4,
        name: "knot_workflow_identity_v1",
        sql: r#"
ALTER TABLE knot_hot ADD COLUMN workflow_id TEXT NOT NULL DEFAULT 'automation_granular';
"#,
    },
    Migration {
        version: 5,
        name: "workflow_id_canonicalize_v1",
        sql: r#"
UPDATE knot_hot
SET workflow_id = 'automation_granular'
WHERE workflow_id IN ('default', 'delivery');
"#,
    },
    Migration {
        version: 6,
        name: "workflow_to_profile_v1",
        sql: r#"
ALTER TABLE knot_hot RENAME COLUMN workflow_id TO profile_id;
ALTER TABLE knot_hot RENAME COLUMN workflow_etag TO profile_etag;
ALTER TABLE knot_hot ADD COLUMN deferred_from_state TEXT;

UPDATE knot_hot
SET profile_id = CASE
    WHEN profile_id IN ('automation_granular', 'default', 'delivery', 'automation', 'granular')
        THEN 'autopilot'
    WHEN profile_id IN ('human_gate', 'human', 'coarse', 'pr_human_gate')
        THEN 'semiauto'
    ELSE profile_id
END;

UPDATE knot_hot
SET state = CASE
    WHEN state = 'idea' THEN 'ready_for_planning'
    WHEN state = 'work_item' THEN 'ready_for_implementation'
    WHEN state = 'implementing' THEN 'implementation'
    WHEN state = 'implemented' THEN 'ready_for_implementation_review'
    WHEN state = 'reviewing' THEN 'implementation_review'
    WHEN state = 'rejected' THEN 'ready_for_implementation'
    WHEN state = 'refining' THEN 'ready_for_implementation'
    WHEN state = 'approved' THEN 'ready_for_shipment'
    ELSE state
END;

UPDATE cold_catalog
SET state = CASE
    WHEN state = 'idea' THEN 'ready_for_planning'
    WHEN state = 'work_item' THEN 'ready_for_implementation'
    WHEN state = 'implementing' THEN 'implementation'
    WHEN state = 'implemented' THEN 'ready_for_implementation_review'
    WHEN state = 'reviewing' THEN 'implementation_review'
    WHEN state = 'rejected' THEN 'ready_for_implementation'
    WHEN state = 'refining' THEN 'ready_for_implementation'
    WHEN state = 'approved' THEN 'ready_for_shipment'
    ELSE state
END;
"#,
    },
    Migration {
        version: 7,
        name: "knot_invariants_v1",
        sql: r#"
ALTER TABLE knot_hot ADD COLUMN invariants_json TEXT NOT NULL DEFAULT '[]';
"#,
    },
    Migration {
        version: 8,
        name: "knot_step_history_v1",
        sql: r#"
ALTER TABLE knot_hot ADD COLUMN step_history_json TEXT NOT NULL DEFAULT '[]';
"#,
    },
    Migration {
        version: 9,
        name: "knot_gate_data_v1",
        sql: r#"
ALTER TABLE knot_hot ADD COLUMN gate_data_json TEXT NOT NULL DEFAULT '{}';
"#,
    },
    Migration {
        version: 10,
        name: "knot_lease_data_v1",
        sql: r#"
ALTER TABLE knot_hot ADD COLUMN lease_data_json TEXT NOT NULL DEFAULT '{}';
ALTER TABLE knot_hot ADD COLUMN lease_id TEXT;
"#,
    },
    Migration {
        version: 11,
        name: "knot_workflow_id_v2",
        sql: r#"
ALTER TABLE knot_hot ADD COLUMN workflow_id TEXT NOT NULL DEFAULT 'compatibility';
"#,
    },
    Migration {
        version: 12,
        name: "knot_acceptance_v1",
        sql: r#"
ALTER TABLE knot_hot ADD COLUMN acceptance TEXT;
"#,
    },
    Migration {
        version: 13,
        name: "knot_blocked_provenance_v1",
        sql: r#"
ALTER TABLE knot_hot ADD COLUMN blocked_from_state TEXT;
"#,
    },
];

pub(super) fn apply_migrations(conn: &mut Connection) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    applied_at TEXT NOT NULL
);
"#,
    )?;

    for migration in MIGRATIONS {
        let already_applied: Option<i64> = tx
            .query_row(
                "SELECT version FROM schema_migrations WHERE version = ?1",
                params![migration.version],
                |row| row.get(0),
            )
            .optional()?;

        if already_applied.is_some() {
            continue;
        }

        tx.execute_batch(migration.sql)?;
        tx.execute(
            "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
            params![migration.version, migration.name, now_utc_rfc3339()],
        )?;
    }

    tx.execute(
        r#"
INSERT INTO meta (key, value)
VALUES ('schema_version', ?1)
ON CONFLICT(key) DO UPDATE SET value = excluded.value
"#,
        params![CURRENT_SCHEMA_VERSION.to_string()],
    )?;
    tx.execute(
        r#"
INSERT INTO meta (key, value)
VALUES ('hot_window_days', '7')
ON CONFLICT(key) DO NOTHING
"#,
        [],
    )?;
    tx.execute(
        r#"
INSERT INTO meta (key, value)
VALUES ('sync_policy', 'auto')
ON CONFLICT(key) DO NOTHING
"#,
        [],
    )?;
    tx.execute(
        r#"
INSERT INTO meta (key, value)
VALUES ('sync_auto_budget_ms', '750')
ON CONFLICT(key) DO NOTHING
"#,
        [],
    )?;
    tx.execute(
        r#"
INSERT INTO meta (key, value)
VALUES ('sync_try_lock_ms', '0')
ON CONFLICT(key) DO NOTHING
"#,
        [],
    )?;
    tx.execute(
        r#"
INSERT INTO meta (key, value)
VALUES ('push_retry_budget_ms', '800')
ON CONFLICT(key) DO NOTHING
"#,
        [],
    )?;
    tx.execute(
        r#"
INSERT INTO meta (key, value)
VALUES ('sync_fetch_blob_limit_kb', '0')
ON CONFLICT(key) DO NOTHING
"#,
        [],
    )?;
    tx.execute(
        r#"
INSERT INTO meta (key, value)
VALUES ('pull_drift_warn_threshold', '25')
ON CONFLICT(key) DO NOTHING
"#,
        [],
    )?;

    tx.commit()
}

pub(super) fn needs_schema_bootstrap(conn: &Connection) -> Result<bool> {
    if !table_exists(conn, "schema_migrations")? || !table_exists(conn, "meta")? {
        return Ok(true);
    }

    let applied_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
            row.get(0)
        })?;
    if applied_count < CURRENT_SCHEMA_VERSION {
        return Ok(true);
    }

    let expected_schema_version = CURRENT_SCHEMA_VERSION.to_string();
    let schema_version = get_meta(conn, "schema_version")?;
    if schema_version.as_deref() != Some(expected_schema_version.as_str()) {
        return Ok(true);
    }

    for key in REQUIRED_META_DEFAULT_KEYS {
        if get_meta(conn, key)?.is_none() {
            return Ok(true);
        }
    }

    Ok(false)
}


fn table_exists(conn: &Connection, table_name: &str) -> Result<bool> {
    let exists: i64 = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        params![table_name],
        |row| row.get(0),
    )?;
    Ok(exists == 1)
}

