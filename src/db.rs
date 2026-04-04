use std::thread;
use std::time::Duration;

use rusqlite::{
    params, types::Type, Connection, DatabaseName, ErrorCode, OptionalExtension, Result,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::domain::gate::GateData;
use crate::domain::invariant::Invariant;
use crate::domain::lease::LeaseData;
use crate::domain::metadata::MetadataEntry;
use crate::domain::step_history::StepRecord;

pub const CURRENT_SCHEMA_VERSION: i64 = 14;

mod catalog;
mod migrations;

pub use catalog::{
    count_active_leases, delete_cold_catalog, delete_edge, delete_knot_warm, get_cold_catalog,
    get_hot_window_days, get_knot_warm, get_pull_drift_warn_threshold,
    get_sync_fetch_blob_limit_kb, insert_edge, list_cold_catalog, list_edges, list_edges_by_kind,
    list_knot_warm, search_cold_catalog, update_lease_expiry_ts, upsert_cold_catalog,
    upsert_knot_warm, EdgeDirection, EdgeRecord,
};

const SQLITE_LOCK_RETRY_LIMIT: usize = 2;
const SQLITE_LOCK_RETRY_BASE_DELAY_MS: u64 = 10;
const SQLITE_LOCK_RETRY_MAX_DELAY_MS: u64 = 250;

#[cfg(test)]
pub fn needs_schema_bootstrap(conn: &rusqlite::Connection) -> Result<bool> {
    migrations::needs_schema_bootstrap(conn)
}

pub fn open_connection(path: &str) -> Result<Connection> {
    let mut conn = Connection::open(path)?;
    configure_for_speed(&conn)?;
    if migrations::needs_schema_bootstrap(&conn)? {
        with_write_retry(|| migrations::apply_migrations(&mut conn))?;
    }
    Ok(conn)
}

/// Open a connection with pragmas but without applying migrations.
/// Used by diagnostics that need to inspect the raw schema state.
pub fn open_connection_raw(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    configure_for_speed(&conn)?;
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

fn with_write_retry<T, F>(mut operation: F) -> Result<T>
where
    F: FnMut() -> Result<T>,
{
    let mut retry = 0usize;
    loop {
        match operation() {
            Ok(value) => return Ok(value),
            Err(err) if is_retryable_lock_error(&err) && retry < SQLITE_LOCK_RETRY_LIMIT => {
                thread::sleep(lock_retry_delay(retry));
                retry += 1;
            }
            Err(err) => return Err(err),
        }
    }
}

fn is_retryable_lock_error(err: &rusqlite::Error) -> bool {
    match err {
        rusqlite::Error::SqliteFailure(sql_err, _) => {
            matches!(
                sql_err.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            )
        }
        _ => false,
    }
}

fn lock_retry_delay(retry: usize) -> Duration {
    let exp = 1u64 << retry.min(5);
    let base = (SQLITE_LOCK_RETRY_BASE_DELAY_MS * exp).min(SQLITE_LOCK_RETRY_MAX_DELAY_MS);
    let jitter = ((retry as u64 * 13) % 7) + 3;
    Duration::from_millis(base + jitter)
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
    pub acceptance: Option<String>,
    pub priority: Option<i64>,
    pub knot_type: Option<String>,
    pub tags: Vec<String>,
    pub notes: Vec<MetadataEntry>,
    pub handoff_capsules: Vec<MetadataEntry>,
    #[serde(default)]
    pub invariants: Vec<Invariant>,
    #[serde(default)]
    pub step_history: Vec<StepRecord>,
    #[serde(default)]
    pub gate_data: GateData,
    #[serde(default)]
    pub lease_data: LeaseData,
    pub lease_id: Option<String>,
    #[serde(default)]
    pub lease_expiry_ts: i64,
    #[serde(default = "default_workflow_id")]
    pub workflow_id: String,
    pub profile_id: String,
    pub profile_etag: Option<String>,
    pub deferred_from_state: Option<String>,
    pub blocked_from_state: Option<String>,
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
    pub acceptance: Option<&'a str>,
    pub priority: Option<i64>,
    pub knot_type: Option<&'a str>,
    pub tags: &'a [String],
    pub notes: &'a [MetadataEntry],
    pub handoff_capsules: &'a [MetadataEntry],
    pub invariants: &'a [Invariant],
    pub step_history: &'a [StepRecord],
    pub gate_data: &'a GateData,
    pub lease_data: &'a LeaseData,
    pub lease_id: Option<&'a str>,
    pub workflow_id: &'a str,
    pub profile_id: &'a str,
    pub profile_etag: Option<&'a str>,
    pub deferred_from_state: Option<&'a str>,
    pub blocked_from_state: Option<&'a str>,
    pub created_at: Option<&'a str>,
}

pub fn upsert_knot_hot(conn: &Connection, args: &UpsertKnotHot<'_>) -> Result<()> {
    let tags_json = to_json_text(args.tags)?;
    let notes_json = to_json_text(args.notes)?;
    let handoff_capsules_json = to_json_text(args.handoff_capsules)?;
    let invariants_json = to_json_text(args.invariants)?;
    let step_history_json = to_json_text(args.step_history)?;
    let gate_data_json = to_json_text(args.gate_data)?;
    let lease_data_json = to_json_text(args.lease_data)?;
    with_write_retry(|| {
        conn.execute(
            r#"
INSERT INTO knot_hot (
    id, title, state, updated_at, body, description, acceptance,
    priority, knot_type, tags_json, notes_json,
    handoff_capsules_json, invariants_json, step_history_json,
    gate_data_json, lease_data_json, lease_id,
    workflow_id, profile_id, profile_etag,
    deferred_from_state, blocked_from_state, created_at
)
VALUES (
    ?1, ?2, ?3, ?4, ?5, ?6, ?7,
    ?8, ?9, ?10, ?11,
    ?12, ?13, ?14, ?15,
    ?16, ?17,
    ?18, ?19, ?20,
    ?21, ?22, ?23
)
ON CONFLICT(id) DO UPDATE SET
    title = excluded.title,
    state = excluded.state,
    updated_at = excluded.updated_at,
    body = excluded.body,
    description = excluded.description,
    acceptance = excluded.acceptance,
    priority = excluded.priority,
    knot_type = excluded.knot_type,
    tags_json = excluded.tags_json,
    notes_json = excluded.notes_json,
    handoff_capsules_json = excluded.handoff_capsules_json,
    invariants_json = excluded.invariants_json,
    step_history_json = excluded.step_history_json,
    gate_data_json = excluded.gate_data_json,
    lease_data_json = excluded.lease_data_json,
    lease_id = excluded.lease_id,
    workflow_id = excluded.workflow_id,
    profile_id = excluded.profile_id,
    profile_etag = excluded.profile_etag,
    deferred_from_state = excluded.deferred_from_state,
    blocked_from_state = excluded.blocked_from_state,
    created_at = COALESCE(knot_hot.created_at, excluded.created_at)
"#,
            params![
                args.id,
                args.title,
                args.state,
                args.updated_at,
                args.body,
                args.description,
                args.acceptance,
                args.priority,
                args.knot_type,
                tags_json.as_str(),
                notes_json.as_str(),
                handoff_capsules_json.as_str(),
                invariants_json.as_str(),
                step_history_json.as_str(),
                gate_data_json.as_str(),
                lease_data_json.as_str(),
                args.lease_id,
                args.workflow_id,
                args.profile_id,
                args.profile_etag,
                args.deferred_from_state,
                args.blocked_from_state,
                args.created_at
            ],
        )?;
        Ok(())
    })?;

    with_write_retry(|| {
        conn.execute("DELETE FROM knot_warm WHERE id = ?1", params![args.id])?;
        Ok(())
    })?;
    Ok(())
}

pub fn get_knot_hot(conn: &Connection, id: &str) -> Result<Option<KnotCacheRecord>> {
    conn.query_row(
        r#"
SELECT id, title, state, updated_at, body, description, acceptance,
       priority, knot_type, tags_json, notes_json,
       handoff_capsules_json, invariants_json, step_history_json,
       gate_data_json, lease_data_json, lease_id, lease_expiry_ts,
       workflow_id, profile_id, profile_etag,
       deferred_from_state, blocked_from_state, created_at
FROM knot_hot
WHERE id = ?1
"#,
        params![id],
        row_to_knot_cache_record,
    )
    .optional()
}

pub fn list_knot_hot(conn: &Connection) -> Result<Vec<KnotCacheRecord>> {
    let mut stmt = conn.prepare(
        r#"
SELECT id, title, state, updated_at, body, description, acceptance,
       priority, knot_type, tags_json, notes_json,
       handoff_capsules_json, invariants_json, step_history_json,
       gate_data_json, lease_data_json, lease_id, lease_expiry_ts,
       workflow_id, profile_id, profile_etag,
       deferred_from_state, blocked_from_state, created_at
FROM knot_hot
ORDER BY updated_at DESC, id ASC
"#,
    )?;

    let mut rows = stmt.query([])?;
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        result.push(row_to_knot_cache_record(row)?);
    }

    Ok(result)
}

fn row_to_knot_cache_record(row: &rusqlite::Row<'_>) -> Result<KnotCacheRecord> {
    let tags_json: String = row.get(9)?;
    let notes_json: String = row.get(10)?;
    let handoff_capsules_json: String = row.get(11)?;
    let invariants_json: String = row.get(12)?;
    let step_history_json: String = row.get(13)?;
    let gate_data_json: String = row.get(14)?;
    let lease_data_json: String = row.get(15)?;
    Ok(KnotCacheRecord {
        id: row.get(0)?,
        title: row.get(1)?,
        state: row.get(2)?,
        updated_at: row.get(3)?,
        body: row.get(4)?,
        description: row.get(5)?,
        acceptance: row.get(6)?,
        priority: row.get(7)?,
        knot_type: row.get(8)?,
        tags: from_json_text(tags_json, 9)?,
        notes: from_json_text(notes_json, 10)?,
        handoff_capsules: from_json_text(handoff_capsules_json, 11)?,
        invariants: from_json_text(invariants_json, 12)?,
        step_history: from_json_text(step_history_json, 13)?,
        gate_data: from_json_text(gate_data_json, 14)?,
        lease_data: from_json_text(lease_data_json, 15)?,
        lease_id: row.get(16)?,
        lease_expiry_ts: row.get(17)?,
        workflow_id: row.get(18)?,
        profile_id: row.get(19)?,
        profile_etag: row.get(20)?,
        deferred_from_state: row.get(21)?,
        blocked_from_state: row.get(22)?,
        created_at: row.get(23)?,
    })
}

pub fn delete_knot_hot(conn: &Connection, id: &str) -> Result<()> {
    with_write_retry(|| {
        conn.execute("DELETE FROM knot_hot WHERE id = ?1", params![id])?;
        Ok(())
    })?;
    Ok(())
}

fn default_workflow_id() -> String {
    "compatibility".to_string()
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
    with_write_retry(|| {
        conn.execute(
            r#"
INSERT INTO meta (key, value)
VALUES (?1, ?2)
ON CONFLICT(key) DO UPDATE SET value = excluded.value
"#,
            params![key, value],
        )?;
        Ok(())
    })?;
    Ok(())
}

#[cfg(test)]
mod tests;
