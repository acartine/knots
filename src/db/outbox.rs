use rusqlite::{params, Connection, OptionalExtension, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use super::now_utc_rfc3339;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct WriterEpoch {
    pub writer_id: String,
    pub credential_id: String,
    pub inbox_ref: String,
    pub expected_inbox_oid: Option<String>,
    pub next_sequence: u64,
    pub registered: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct OutboxRecord {
    pub event_id: String,
    pub stream: String,
    pub relative_path: String,
    pub content_sha256: String,
    pub payload: Vec<u8>,
    pub writer_id: Option<String>,
    pub sequence: Option<u64>,
    pub proposed_inbox_oid: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct LegacyQuarantineRecord {
    pub relative_path: String,
    pub content_sha256: String,
}

pub(crate) fn record_outbox_event(
    conn: &Connection,
    event_id: &str,
    stream: &str,
    relative_path: &str,
    content_sha256: &str,
    payload: &[u8],
) -> Result<()> {
    let existing: Option<(String, Vec<u8>)> = conn
        .query_row(
            "SELECT content_sha256, payload FROM v2_outbox WHERE event_id = ?1",
            [event_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if let Some((hash, bytes)) = existing {
        if hash == content_sha256 && bytes == payload {
            return Ok(());
        }
        return Err(rusqlite::Error::InvalidQuery);
    }
    conn.execute(
        "INSERT INTO v2_outbox (
            event_id, stream, relative_path, content_sha256, payload, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            event_id,
            stream,
            relative_path,
            content_sha256,
            payload,
            now_utc_rfc3339()
        ],
    )?;
    Ok(())
}

pub(crate) fn ensure_writer_epoch(conn: &Connection, credential_id: &str) -> Result<WriterEpoch> {
    let identity = super::ensure_writer_identity_for_credential(conn, Some(credential_id))
        .map_err(identity_error)?;
    if let Some(writer) = active_writer(conn)? {
        if writer.writer_id == identity.writer_id && writer.credential_id == credential_id {
            return Ok(writer);
        }
        let pending: bool = conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM v2_outbox
                WHERE writer_id = ?1 AND acknowledged_at IS NULL
            )",
            [&writer.writer_id],
            |row| row.get(0),
        )?;
        if pending {
            return Err(rusqlite::Error::InvalidQuery);
        }
        return rotate_writer_epoch(conn, credential_id);
    }
    Err(rusqlite::Error::InvalidQuery)
}

pub(crate) fn rotate_writer_epoch(conn: &Connection, credential_id: &str) -> Result<WriterEpoch> {
    super::ensure_writer_identity(conn).map_err(identity_error)?;
    super::rotate_writer_identity_for_credential(conn, Some(credential_id))
        .map_err(identity_error)?;
    active_writer(conn)?.ok_or(rusqlite::Error::InvalidQuery)
}

fn identity_error(error: super::WriterIdentityError) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(error))
}

fn active_writer(conn: &Connection) -> Result<Option<WriterEpoch>> {
    conn.query_row(
        "SELECT writer_id, credential_id, inbox_ref, expected_inbox_oid,
                next_sequence, registered
         FROM v2_writer_epoch WHERE active = 1",
        [],
        map_writer,
    )
    .optional()
}

fn map_writer(row: &rusqlite::Row<'_>) -> Result<WriterEpoch> {
    Ok(WriterEpoch {
        writer_id: row.get(0)?,
        credential_id: row.get(1)?,
        inbox_ref: row.get(2)?,
        expected_inbox_oid: row.get(3)?,
        next_sequence: row.get::<_, i64>(4)? as u64,
        registered: row.get::<_, i64>(5)? != 0,
    })
}

pub(crate) fn assign_pending_outbox(
    conn: &Connection,
    writer: &WriterEpoch,
    limit: usize,
) -> Result<Vec<OutboxRecord>> {
    let existing = list_pending_outbox(conn, &writer.writer_id, limit)?;
    if !existing.is_empty() {
        return Ok(existing);
    }
    let tx = conn.unchecked_transaction()?;
    let mut rows = list_unassigned(&tx, limit)?;
    let mut sequence = writer.next_sequence;
    for row in &mut rows {
        tx.execute(
            "UPDATE v2_outbox SET writer_id = ?1, sequence = ?2
             WHERE event_id = ?3 AND writer_id IS NULL",
            params![writer.writer_id, sequence as i64, row.event_id],
        )?;
        row.writer_id = Some(writer.writer_id.clone());
        row.sequence = Some(sequence);
        sequence += 1;
    }
    tx.execute(
        "UPDATE v2_writer_epoch SET next_sequence = ?1 WHERE writer_id = ?2",
        params![sequence as i64, writer.writer_id],
    )?;
    tx.commit()?;
    list_pending_outbox(conn, &writer.writer_id, limit)
}

fn list_unassigned(conn: &Connection, limit: usize) -> Result<Vec<OutboxRecord>> {
    query_outbox(
        conn,
        "SELECT event_id, stream, relative_path, content_sha256, payload,
                writer_id, sequence, proposed_inbox_oid
         FROM v2_outbox WHERE writer_id IS NULL AND acknowledged_at IS NULL
         ORDER BY created_at, event_id LIMIT ?1",
        None,
        limit,
    )
}

pub(crate) fn list_pending_outbox(
    conn: &Connection,
    writer_id: &str,
    limit: usize,
) -> Result<Vec<OutboxRecord>> {
    query_outbox(
        conn,
        "SELECT event_id, stream, relative_path, content_sha256, payload,
                writer_id, sequence, proposed_inbox_oid
         FROM v2_outbox WHERE writer_id = ?1 AND acknowledged_at IS NULL
         ORDER BY sequence LIMIT ?2",
        Some(writer_id),
        limit,
    )
}

fn query_outbox(
    conn: &Connection,
    sql: &str,
    writer_id: Option<&str>,
    limit: usize,
) -> Result<Vec<OutboxRecord>> {
    let mut statement = conn.prepare(sql)?;
    let mapper = |row: &rusqlite::Row<'_>| {
        Ok(OutboxRecord {
            event_id: row.get(0)?,
            stream: row.get(1)?,
            relative_path: row.get(2)?,
            content_sha256: row.get(3)?,
            payload: row.get(4)?,
            writer_id: row.get(5)?,
            sequence: row.get::<_, Option<i64>>(6)?.map(|value| value as u64),
            proposed_inbox_oid: row.get(7)?,
        })
    };
    let rows = if let Some(writer_id) = writer_id {
        statement.query_map(params![writer_id, limit as i64], mapper)?
    } else {
        statement.query_map([limit as i64], mapper)?
    };
    rows.collect()
}

pub(crate) fn mark_outbox_proposed(
    conn: &Connection,
    writer_id: &str,
    event_ids: &[String],
    proposed_oid: &str,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for event_id in event_ids {
        let changed = tx.execute(
            "UPDATE v2_outbox SET proposed_inbox_oid = ?1
             WHERE event_id = ?2 AND writer_id = ?3 AND acknowledged_at IS NULL
               AND (proposed_inbox_oid IS NULL OR proposed_inbox_oid = ?1)",
            params![proposed_oid, event_id, writer_id],
        )?;
        if changed != 1 {
            return Err(rusqlite::Error::InvalidQuery);
        }
    }
    tx.commit()
}

pub(crate) fn acknowledge_outbox(
    conn: &Connection,
    writer_id: &str,
    proposed_oid: &str,
    registered: bool,
) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let changed = tx.execute(
        "UPDATE v2_outbox
         SET acknowledged_inbox_oid = ?1, acknowledged_at = ?2
         WHERE writer_id = ?3 AND proposed_inbox_oid = ?1 AND acknowledged_at IS NULL",
        params![proposed_oid, now_utc_rfc3339(), writer_id],
    )?;
    tx.execute(
        "UPDATE v2_writer_epoch
         SET expected_inbox_oid = ?1, registered = MAX(registered, ?2)
         WHERE writer_id = ?3",
        params![proposed_oid, i64::from(registered), writer_id],
    )?;
    tx.commit()?;
    Ok(changed)
}

pub(crate) fn record_legacy_quarantine(
    conn: &Connection,
    record: &LegacyQuarantineRecord,
) -> Result<()> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT content_sha256 FROM v2_legacy_quarantine WHERE relative_path = ?1",
            [&record.relative_path],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(hash) = existing {
        return if hash == record.content_sha256 {
            Ok(())
        } else {
            Err(rusqlite::Error::InvalidQuery)
        };
    }
    conn.execute(
        "INSERT INTO v2_legacy_quarantine (
            relative_path, content_sha256, inventoried_at
         ) VALUES (?1, ?2, ?3)",
        params![
            record.relative_path,
            record.content_sha256,
            now_utc_rfc3339()
        ],
    )?;
    Ok(())
}

pub(crate) fn inventory_legacy_files(conn: &Connection, store_root: &Path) -> Result<usize> {
    let mut files = Vec::new();
    for directory in ["events", "index"] {
        collect_json_files(&store_root.join(directory), &mut files)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    }
    files.sort();
    for path in &files {
        let payload = std::fs::read(path)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        let relative = path
            .strip_prefix(store_root)
            .map_err(|_| rusqlite::Error::InvalidPath(path.clone()))?;
        let relative_text = relative.to_string_lossy().into_owned();
        let locally_authored: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM v2_outbox WHERE relative_path = ?1)",
            [&relative_text],
            |row| row.get(0),
        )?;
        if locally_authored {
            continue;
        }
        record_legacy_quarantine(
            conn,
            &LegacyQuarantineRecord {
                relative_path: relative_text,
                content_sha256: format!("{:x}", Sha256::digest(payload)),
            },
        )?;
    }
    Ok(files.len())
}

fn collect_json_files(root: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !root.exists() {
        return Ok(());
    }
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            if path.is_dir() {
                directories.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                files.push(path);
            }
        }
    }
    Ok(())
}
