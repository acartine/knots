use rusqlite::{params, Connection, OptionalExtension};

use crate::events::now_utc_rfc3339;

use super::errors::ImportError;

pub fn load_checkpoint(conn: &Connection, source_key: &str) -> Result<Option<usize>, ImportError> {
    let checkpoint: Option<Option<String>> = conn
        .query_row(
            "SELECT checkpoint FROM import_state WHERE source_key = ?1",
            params![source_key],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?;
    Ok(checkpoint
        .flatten()
        .and_then(|value| value.parse::<usize>().ok()))
}

pub fn fingerprint_exists(conn: &Connection, token: &str) -> Result<bool, ImportError> {
    let exists: Option<String> = conn
        .query_row(
            "SELECT fingerprint FROM import_fingerprints WHERE fingerprint = ?1",
            params![token],
            |row| row.get(0),
        )
        .optional()?;
    Ok(exists.is_some())
}

pub fn insert_fingerprint(
    conn: &Connection,
    token: &str,
    source_key: &str,
    knot_id: &str,
    occurred_at: &str,
    action: &str,
) -> Result<(), ImportError> {
    conn.execute(
        concat!(
            "INSERT INTO import_fingerprints ",
            "(fingerprint, source_key, knot_id, occurred_at, action, created_at) ",
            "VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
        ),
        params![
            token,
            source_key,
            knot_id,
            occurred_at,
            action,
            now_utc_rfc3339()
        ],
    )?;
    Ok(())
}
