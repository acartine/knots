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

pub fn count_active_leases(conn: &Connection) -> Result<i64> {
    conn.query_row(
        r#"
SELECT COUNT(*) FROM knot_hot
WHERE knot_type = 'lease'
  AND state IN ('lease_ready', 'lease_active')
"#,
        [],
        |row| row.get(0),
    )
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
