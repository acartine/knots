use std::time::Duration;

use rusqlite::{params, Connection, DatabaseName, OptionalExtension, Result};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub const CURRENT_SCHEMA_VERSION: i64 = 2;

struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: [Migration; 2] = [
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
        name: "import_tracking_v1",
        sql: r#"
CREATE TABLE IF NOT EXISTS import_state (
    source_key TEXT PRIMARY KEY,
    source_type TEXT NOT NULL,
    source_ref TEXT NOT NULL,
    last_run_at TEXT NOT NULL,
    last_status TEXT NOT NULL,
    processed_count INTEGER NOT NULL DEFAULT 0,
    imported_count INTEGER NOT NULL DEFAULT 0,
    skipped_count INTEGER NOT NULL DEFAULT 0,
    error_count INTEGER NOT NULL DEFAULT 0,
    checkpoint TEXT,
    last_error TEXT
);

CREATE TABLE IF NOT EXISTS import_fingerprints (
    fingerprint TEXT PRIMARY KEY,
    source_key TEXT NOT NULL,
    knot_id TEXT NOT NULL,
    occurred_at TEXT NOT NULL,
    action TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_import_fingerprints_source_key
    ON import_fingerprints(source_key);
"#,
    },
];

pub fn open_connection(path: &str) -> Result<Connection> {
    let mut conn = Connection::open(path)?;
    configure_for_speed(&conn)?;
    apply_migrations(&mut conn)?;
    Ok(conn)
}

fn configure_for_speed(conn: &Connection) -> Result<()> {
    conn.pragma_update(None::<DatabaseName>, "journal_mode", "WAL")?;
    conn.pragma_update(None::<DatabaseName>, "synchronous", "NORMAL")?;
    conn.pragma_update(None::<DatabaseName>, "foreign_keys", "ON")?;
    conn.pragma_update(None::<DatabaseName>, "temp_store", "MEMORY")?;
    conn.pragma_update(None::<DatabaseName>, "busy_timeout", 5000i64)?;
    conn.busy_timeout(Duration::from_millis(5000))?;
    Ok(())
}

fn apply_migrations(conn: &mut Connection) -> Result<()> {
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

    tx.commit()
}

fn now_utc_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .expect("RFC3339 formatting for UTC timestamp should never fail")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnotCacheRecord {
    pub id: String,
    pub title: String,
    pub state: String,
    pub updated_at: String,
    pub body: Option<String>,
    pub workflow_etag: Option<String>,
    pub created_at: Option<String>,
}

pub struct UpsertKnotHot<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub state: &'a str,
    pub updated_at: &'a str,
    pub body: Option<&'a str>,
    pub workflow_etag: Option<&'a str>,
    pub created_at: Option<&'a str>,
}

pub fn upsert_knot_hot(conn: &Connection, args: &UpsertKnotHot<'_>) -> Result<()> {
    conn.execute(
        r#"
INSERT INTO knot_hot (
    id, title, state, updated_at, body, workflow_etag, created_at
)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
ON CONFLICT(id) DO UPDATE SET
    title = excluded.title,
    state = excluded.state,
    updated_at = excluded.updated_at,
    body = excluded.body,
    workflow_etag = excluded.workflow_etag,
    created_at = COALESCE(knot_hot.created_at, excluded.created_at)
"#,
        params![
            args.id,
            args.title,
            args.state,
            args.updated_at,
            args.body,
            args.workflow_etag,
            args.created_at
        ],
    )?;

    conn.execute("DELETE FROM knot_warm WHERE id = ?1", params![args.id])?;
    Ok(())
}

pub fn get_knot_hot(conn: &Connection, id: &str) -> Result<Option<KnotCacheRecord>> {
    conn.query_row(
        r#"
SELECT id, title, state, updated_at, body, workflow_etag, created_at
FROM knot_hot
WHERE id = ?1
"#,
        params![id],
        |row| {
            Ok(KnotCacheRecord {
                id: row.get(0)?,
                title: row.get(1)?,
                state: row.get(2)?,
                updated_at: row.get(3)?,
                body: row.get(4)?,
                workflow_etag: row.get(5)?,
                created_at: row.get(6)?,
            })
        },
    )
    .optional()
}

pub fn list_knot_hot(conn: &Connection) -> Result<Vec<KnotCacheRecord>> {
    let mut stmt = conn.prepare(
        r#"
SELECT id, title, state, updated_at, body, workflow_etag, created_at
FROM knot_hot
ORDER BY updated_at DESC, id ASC
"#,
    )?;

    let mut rows = stmt.query([])?;
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        result.push(KnotCacheRecord {
            id: row.get(0)?,
            title: row.get(1)?,
            state: row.get(2)?,
            updated_at: row.get(3)?,
            body: row.get(4)?,
            workflow_etag: row.get(5)?,
            created_at: row.get(6)?,
        });
    }

    Ok(result)
}

pub fn delete_knot_hot(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM knot_hot WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn delete_knot_warm(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM knot_warm WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn get_meta(conn: &Connection, key: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT value FROM meta WHERE key = ?1",
        params![key],
        |row| row.get(0),
    )
    .optional()
}

pub fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        r#"
INSERT INTO meta (key, value)
VALUES (?1, ?2)
ON CONFLICT(key) DO UPDATE SET value = excluded.value
"#,
        params![key, value],
    )?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeRecord {
    pub src: String,
    pub kind: String,
    pub dst: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeDirection {
    Incoming,
    Outgoing,
    Both,
}

pub fn insert_edge(conn: &Connection, src: &str, kind: &str, dst: &str) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO edge (src, kind, dst) VALUES (?1, ?2, ?3)",
        params![src, kind, dst],
    )?;
    Ok(())
}

pub fn delete_edge(conn: &Connection, src: &str, kind: &str, dst: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM edge WHERE src = ?1 AND kind = ?2 AND dst = ?3",
        params![src, kind, dst],
    )?;
    Ok(())
}

pub fn list_edges(
    conn: &Connection,
    knot_id: &str,
    direction: EdgeDirection,
) -> Result<Vec<EdgeRecord>> {
    let sql = match direction {
        EdgeDirection::Incoming => {
            "SELECT src, kind, dst FROM edge WHERE dst = ?1 ORDER BY src, kind, dst"
        }
        EdgeDirection::Outgoing => {
            "SELECT src, kind, dst FROM edge WHERE src = ?1 ORDER BY src, kind, dst"
        }
        EdgeDirection::Both => {
            "SELECT src, kind, dst FROM edge WHERE src = ?1 OR dst = ?1 ORDER BY src, kind, dst"
        }
    };
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query(params![knot_id])?;
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        result.push(EdgeRecord {
            src: row.get(0)?,
            kind: row.get(1)?,
            dst: row.get(2)?,
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
