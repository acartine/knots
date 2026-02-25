use std::time::Duration;

use rusqlite::{params, types::Type, Connection, DatabaseName, OptionalExtension, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::domain::metadata::MetadataEntry;

pub const CURRENT_SCHEMA_VERSION: i64 = 5;

struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: [Migration; 5] = [
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

    tx.commit()
}

fn now_utc_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .expect("RFC3339 formatting for UTC timestamp should never fail")
}

fn to_json_text<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    serde_json::to_string(value)
        .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))
}

fn from_json_text<T: DeserializeOwned>(raw: String, column: usize) -> Result<T> {
    serde_json::from_str(&raw)
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(column, Type::Text, Box::new(err)))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KnotCacheRecord {
    pub id: String,
    pub title: String,
    pub state: String,
    pub updated_at: String,
    pub body: Option<String>,
    pub description: Option<String>,
    pub priority: Option<i64>,
    pub knot_type: Option<String>,
    pub tags: Vec<String>,
    pub notes: Vec<MetadataEntry>,
    pub handoff_capsules: Vec<MetadataEntry>,
    pub workflow_id: String,
    pub workflow_etag: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WarmKnotRecord {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ColdCatalogRecord {
    pub id: String,
    pub title: String,
    pub state: String,
    pub updated_at: String,
}

pub struct UpsertKnotHot<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub state: &'a str,
    pub updated_at: &'a str,
    pub body: Option<&'a str>,
    pub description: Option<&'a str>,
    pub priority: Option<i64>,
    pub knot_type: Option<&'a str>,
    pub tags: &'a [String],
    pub notes: &'a [MetadataEntry],
    pub handoff_capsules: &'a [MetadataEntry],
    pub workflow_id: &'a str,
    pub workflow_etag: Option<&'a str>,
    pub created_at: Option<&'a str>,
}

pub fn upsert_knot_hot(conn: &Connection, args: &UpsertKnotHot<'_>) -> Result<()> {
    let tags_json = to_json_text(args.tags)?;
    let notes_json = to_json_text(args.notes)?;
    let handoff_capsules_json = to_json_text(args.handoff_capsules)?;
    conn.execute(
        r#"
INSERT INTO knot_hot (
    id, title, state, updated_at, body, description, priority, knot_type,
    tags_json, notes_json, handoff_capsules_json, workflow_id, workflow_etag, created_at
)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
ON CONFLICT(id) DO UPDATE SET
    title = excluded.title,
    state = excluded.state,
    updated_at = excluded.updated_at,
    body = excluded.body,
    description = excluded.description,
    priority = excluded.priority,
    knot_type = excluded.knot_type,
    tags_json = excluded.tags_json,
    notes_json = excluded.notes_json,
    handoff_capsules_json = excluded.handoff_capsules_json,
    workflow_id = excluded.workflow_id,
    workflow_etag = excluded.workflow_etag,
    created_at = COALESCE(knot_hot.created_at, excluded.created_at)
"#,
        params![
            args.id,
            args.title,
            args.state,
            args.updated_at,
            args.body,
            args.description,
            args.priority,
            args.knot_type,
            tags_json,
            notes_json,
            handoff_capsules_json,
            args.workflow_id,
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
SELECT id, title, state, updated_at, body, description, priority, knot_type,
       tags_json, notes_json, handoff_capsules_json, workflow_id, workflow_etag, created_at
FROM knot_hot
WHERE id = ?1
"#,
        params![id],
        |row| {
            let tags_json: String = row.get(8)?;
            let notes_json: String = row.get(9)?;
            let handoff_capsules_json: String = row.get(10)?;
            Ok(KnotCacheRecord {
                id: row.get(0)?,
                title: row.get(1)?,
                state: row.get(2)?,
                updated_at: row.get(3)?,
                body: row.get(4)?,
                description: row.get(5)?,
                priority: row.get(6)?,
                knot_type: row.get(7)?,
                tags: from_json_text(tags_json, 8)?,
                notes: from_json_text(notes_json, 9)?,
                handoff_capsules: from_json_text(handoff_capsules_json, 10)?,
                workflow_id: row.get(11)?,
                workflow_etag: row.get(12)?,
                created_at: row.get(13)?,
            })
        },
    )
    .optional()
}

pub fn list_knot_hot(conn: &Connection) -> Result<Vec<KnotCacheRecord>> {
    let mut stmt = conn.prepare(
        r#"
SELECT id, title, state, updated_at, body, description, priority, knot_type,
       tags_json, notes_json, handoff_capsules_json, workflow_id, workflow_etag, created_at
FROM knot_hot
ORDER BY updated_at DESC, id ASC
"#,
    )?;

    let mut rows = stmt.query([])?;
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        let tags_json: String = row.get(8)?;
        let notes_json: String = row.get(9)?;
        let handoff_capsules_json: String = row.get(10)?;
        result.push(KnotCacheRecord {
            id: row.get(0)?,
            title: row.get(1)?,
            state: row.get(2)?,
            updated_at: row.get(3)?,
            body: row.get(4)?,
            description: row.get(5)?,
            priority: row.get(6)?,
            knot_type: row.get(7)?,
            tags: from_json_text(tags_json, 8)?,
            notes: from_json_text(notes_json, 9)?,
            handoff_capsules: from_json_text(handoff_capsules_json, 10)?,
            workflow_id: row.get(11)?,
            workflow_etag: row.get(12)?,
            created_at: row.get(13)?,
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

pub fn upsert_knot_warm(conn: &Connection, id: &str, title: &str) -> Result<()> {
    conn.execute(
        r#"
INSERT INTO knot_warm (id, title)
VALUES (?1, ?2)
ON CONFLICT(id) DO UPDATE SET title = excluded.title
"#,
        params![id, title],
    )?;
    Ok(())
}

pub fn get_knot_warm(conn: &Connection, id: &str) -> Result<Option<WarmKnotRecord>> {
    conn.query_row(
        "SELECT id, title FROM knot_warm WHERE id = ?1",
        params![id],
        |row| {
            Ok(WarmKnotRecord {
                id: row.get(0)?,
                title: row.get(1)?,
            })
        },
    )
    .optional()
}

pub fn list_knot_warm(conn: &Connection) -> Result<Vec<WarmKnotRecord>> {
    let mut stmt = conn.prepare("SELECT id, title FROM knot_warm ORDER BY id ASC")?;
    let mut rows = stmt.query([])?;
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        result.push(WarmKnotRecord {
            id: row.get(0)?,
            title: row.get(1)?,
        });
    }
    Ok(result)
}

pub fn upsert_cold_catalog(
    conn: &Connection,
    id: &str,
    title: &str,
    state: &str,
    updated_at: &str,
) -> Result<()> {
    conn.execute(
        r#"
INSERT INTO cold_catalog (id, title, state, updated_at)
VALUES (?1, ?2, ?3, ?4)
ON CONFLICT(id) DO UPDATE SET
    title = excluded.title,
    state = excluded.state,
    updated_at = excluded.updated_at
"#,
        params![id, title, state, updated_at],
    )?;
    Ok(())
}

pub fn get_cold_catalog(conn: &Connection, id: &str) -> Result<Option<ColdCatalogRecord>> {
    conn.query_row(
        "SELECT id, title, state, updated_at FROM cold_catalog WHERE id = ?1",
        params![id],
        |row| {
            Ok(ColdCatalogRecord {
                id: row.get(0)?,
                title: row.get(1)?,
                state: row.get(2)?,
                updated_at: row.get(3)?,
            })
        },
    )
    .optional()
}

pub fn search_cold_catalog(conn: &Connection, term: &str) -> Result<Vec<ColdCatalogRecord>> {
    let like = format!("%{}%", term.trim());
    let mut stmt = conn.prepare(
        r#"
SELECT id, title, state, updated_at
FROM cold_catalog
WHERE id LIKE ?1 OR title LIKE ?1
ORDER BY updated_at DESC, id ASC
"#,
    )?;
    let mut rows = stmt.query(params![like])?;
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        result.push(ColdCatalogRecord {
            id: row.get(0)?,
            title: row.get(1)?,
            state: row.get(2)?,
            updated_at: row.get(3)?,
        });
    }
    Ok(result)
}

pub fn list_cold_catalog(conn: &Connection) -> Result<Vec<ColdCatalogRecord>> {
    let mut stmt = conn.prepare(
        r#"
SELECT id, title, state, updated_at
FROM cold_catalog
ORDER BY updated_at DESC, id ASC
"#,
    )?;
    let mut rows = stmt.query([])?;
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        result.push(ColdCatalogRecord {
            id: row.get(0)?,
            title: row.get(1)?,
            state: row.get(2)?,
            updated_at: row.get(3)?,
        });
    }
    Ok(result)
}

pub fn delete_cold_catalog(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM cold_catalog WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn get_hot_window_days(conn: &Connection) -> Result<i64> {
    let value = get_meta(conn, "hot_window_days")?;
    let parsed = value
        .as_deref()
        .unwrap_or("7")
        .trim()
        .parse::<i64>()
        .unwrap_or(7);
    Ok(parsed.max(0))
}

pub fn get_sync_fetch_blob_limit_kb(conn: &Connection) -> Result<Option<u64>> {
    if let Ok(raw) = std::env::var("KNOTS_FETCH_BLOB_LIMIT_KB") {
        let parsed = raw.trim().parse::<u64>().unwrap_or(0);
        if parsed > 0 {
            return Ok(Some(parsed));
        }
    }

    let value = get_meta(conn, "sync_fetch_blob_limit_kb")?;
    let parsed = value
        .as_deref()
        .unwrap_or("0")
        .trim()
        .parse::<u64>()
        .unwrap_or(0);
    if parsed > 0 {
        Ok(Some(parsed))
    } else {
        Ok(None)
    }
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

pub fn list_edges_by_kind(conn: &Connection, kind: &str) -> Result<Vec<EdgeRecord>> {
    let mut stmt =
        conn.prepare("SELECT src, kind, dst FROM edge WHERE kind = ?1 ORDER BY src ASC, dst ASC")?;
    let mut rows = stmt.query(params![kind])?;
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
