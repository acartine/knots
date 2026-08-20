use rusqlite::{params, Connection, OptionalExtension, Result};

use super::{with_write_retry, ColdCatalogRecord, WarmKnotRecord};

pub fn delete_knot_warm(conn: &Connection, id: &str) -> Result<()> {
    with_write_retry(|| {
        conn.execute("DELETE FROM knot_warm WHERE id = ?1", params![id])?;
        Ok(())
    })?;
    Ok(())
}

pub fn upsert_knot_warm(conn: &Connection, id: &str, title: &str) -> Result<()> {
    with_write_retry(|| {
        conn.execute(
            r#"
INSERT INTO knot_warm (id, title)
VALUES (?1, ?2)
ON CONFLICT(id) DO UPDATE SET title = excluded.title
"#,
            params![id, title],
        )?;
        Ok(())
    })?;
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
    with_write_retry(|| {
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
    })?;
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

/// Count cold_catalog rows whose `state` is not in `terminal_states`. Real
/// healthy cold rows are always terminal — the sweep filters by terminal,
/// and sync only routes to cold on terminal events. A non-terminal cold row
/// is therefore a misclassification that doctor should surface.
pub fn count_non_terminal_in_cold(conn: &Connection, terminal_states: &[&str]) -> Result<i64> {
    let placeholders = vec!["?"; terminal_states.len()].join(",");
    let sql = format!("SELECT COUNT(*) FROM cold_catalog WHERE state NOT IN ({placeholders})");
    let params: Vec<&dyn rusqlite::ToSql> = terminal_states
        .iter()
        .map(|s| s as &dyn rusqlite::ToSql)
        .collect();
    conn.query_row(&sql, params.as_slice(), |row| row.get(0))
}

/// Cold_catalog rows whose `state` is not terminal. Used by doctor --fix to
/// rehydrate the row back to hot (or, if events are missing, drop the cold
/// pointer so the warning can clear).
pub fn list_non_terminal_in_cold(
    conn: &Connection,
    terminal_states: &[&str],
) -> Result<Vec<ColdCatalogRecord>> {
    let placeholders = vec!["?"; terminal_states.len()].join(",");
    let sql = format!(
        "SELECT id, title, state, updated_at FROM cold_catalog \
         WHERE state NOT IN ({placeholders}) \
         ORDER BY updated_at DESC, id ASC"
    );
    let params: Vec<&dyn rusqlite::ToSql> = terminal_states
        .iter()
        .map(|s| s as &dyn rusqlite::ToSql)
        .collect();
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params.as_slice())?;
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

/// Count `knot_hot` rows that are simultaneously terminal AND older than the
/// archive cutoff. The cold sweep handles these when hot exceeds HOT_HIGH_WATER,
/// but if hot is below HOT_HIGH_WATER they can persist — for example after
/// `doctor --fix` rehydrated stale-terminal rows in a prior version. Doctor
/// surfaces and demotes them so steady state holds the invariant.
pub fn count_stale_terminal_in_hot(
    conn: &Connection,
    terminal_states: &[&str],
    cutoff_rfc3339: &str,
) -> Result<i64> {
    let placeholders = vec!["?"; terminal_states.len()].join(",");
    let sql = format!(
        "SELECT COUNT(*) FROM knot_hot \
         WHERE state IN ({placeholders}) AND updated_at < ?"
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = terminal_states
        .iter()
        .map(|s| s as &dyn rusqlite::ToSql)
        .collect();
    params.push(&cutoff_rfc3339);
    conn.query_row(&sql, params.as_slice(), |row| row.get(0))
}

/// Knot_hot rows that should have been swept to cold by archival. Returns the
/// minimal fields needed to perform an `upsert_cold_catalog` + `delete_knot_hot`
/// pair (mirroring the sweep's `move_candidates_in_tx`).
#[allow(clippy::type_complexity)]
pub fn list_stale_terminal_in_hot(
    conn: &Connection,
    terminal_states: &[&str],
    cutoff_rfc3339: &str,
) -> Result<Vec<(String, String, String, String)>> {
    let placeholders = vec!["?"; terminal_states.len()].join(",");
    let sql = format!(
        "SELECT id, title, state, updated_at FROM knot_hot \
         WHERE state IN ({placeholders}) AND updated_at < ? \
         ORDER BY updated_at ASC, id ASC"
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = terminal_states
        .iter()
        .map(|s| s as &dyn rusqlite::ToSql)
        .collect();
    params.push(&cutoff_rfc3339);
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params.as_slice())?;
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        result.push((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?));
    }
    Ok(result)
}

/// Count of cold-catalog rows whose id is also present in `knot_hot`. These
/// are data-consistency leftovers that should be evicted from cold.
pub fn count_cold_catalog_shadowed_by_hot(conn: &Connection) -> Result<i64> {
    conn.query_row(
        r#"
SELECT COUNT(*) FROM cold_catalog c
WHERE EXISTS (SELECT 1 FROM knot_hot h WHERE h.id = c.id)
"#,
        [],
        |row| row.get(0),
    )
}

/// Delete every cold_catalog row whose id is already present in knot_hot.
/// Returns the number of rows removed.
pub fn prune_cold_catalog_shadowed_by_hot(conn: &Connection) -> Result<usize> {
    let removed = with_write_retry(|| {
        let n = conn.execute(
            r#"
DELETE FROM cold_catalog
WHERE id IN (SELECT h.id FROM knot_hot h WHERE h.id = cold_catalog.id)
"#,
            [],
        )?;
        Ok(n)
    })?;
    Ok(removed)
}

pub fn delete_cold_catalog(conn: &Connection, id: &str) -> Result<()> {
    with_write_retry(|| {
        conn.execute("DELETE FROM cold_catalog WHERE id = ?1", params![id])?;
        Ok(())
    })?;
    Ok(())
}

pub fn get_hot_window_days(conn: &Connection) -> Result<i64> {
    let value = super::get_meta(conn, "hot_window_days")?;
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

    let value = super::get_meta(conn, "sync_fetch_blob_limit_kb")?;
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

pub fn get_pull_drift_warn_threshold(conn: &Connection) -> Result<u64> {
    let value = super::get_meta(conn, "pull_drift_warn_threshold")?;
    let parsed = value
        .as_deref()
        .unwrap_or("25")
        .trim()
        .parse::<u64>()
        .unwrap_or(25);
    Ok(parsed)
}

pub fn count_cold_catalog(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM cold_catalog", [], |row| row.get(0))
}

pub fn count_knot_hot(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM knot_hot", [], |row| row.get(0))
}

pub(super) const ACTIVE_LEASE_PREDICATE: &str = r#"
WHERE knot_type = 'lease'
  AND state IN ('lease_ready', 'lease_active')
  AND lease_expiry_ts > unixepoch('now')
"#;

/// Leases owned by `?1`. A lease with no recorded owner predates owner
/// tracking and counts as local, so existing stores keep today's behavior.
pub(super) const LOCAL_LEASE_PREDICATE: &str =
    "  AND COALESCE(json_extract(lease_data_json, '$.owner.machine_id'), ?1) = ?1\n";

pub fn count_active_leases(conn: &Connection) -> Result<i64> {
    let sql = format!("SELECT COUNT(*) FROM knot_hot{}", ACTIVE_LEASE_PREDICATE);
    conn.query_row(&sql, [], |row| row.get(0))
}

pub fn update_lease_expiry_ts(conn: &Connection, id: &str, ts: i64) -> Result<()> {
    super::with_write_retry(|| {
        conn.execute(
            "UPDATE knot_hot SET lease_expiry_ts = ?1 WHERE id = ?2",
            params![ts, id],
        )?;
        Ok(())
    })
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
    with_write_retry(|| {
        conn.execute(
            "INSERT OR IGNORE INTO edge (src, kind, dst) VALUES (?1, ?2, ?3)",
            params![src, kind, dst],
        )?;
        Ok(())
    })?;
    Ok(())
}

pub fn delete_edge(conn: &Connection, src: &str, kind: &str, dst: &str) -> Result<()> {
    with_write_retry(|| {
        conn.execute(
            "DELETE FROM edge WHERE src = ?1 AND kind = ?2 AND dst = ?3",
            params![src, kind, dst],
        )?;
        Ok(())
    })?;
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
