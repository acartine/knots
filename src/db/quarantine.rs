//! Per-knot quarantine for pull.
//!
//! When a pull would apply an event for a knot this machine currently holds
//! a local lease on, it skips that event instead and records a row here:
//! `(knot_id, base_commit)`, where `base_commit` is the watermark just
//! before the pull that first skipped it. The row persists until a later
//! pull finds the knot no longer locally leased, at which point it replays
//! everything from `base_commit` forward and drops the row.
//!
//! Recording `base_commit` once (insert-or-ignore) is what keeps this safe:
//! the shared `last_index_head_commit` / `last_full_head_commit` watermark
//! still advances every pass, but a quarantined knot's own catch-up point
//! stays pinned to the moment it was first held back, so none of its events
//! are lost to the advancing watermark.

use std::collections::HashSet;

use rusqlite::{params, Connection, Result};

use super::catalog::{ACTIVE_LEASE_PREDICATE, LOCAL_LEASE_PREDICATE};
use super::{now_utc_rfc3339, with_write_retry};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuarantinedKnot {
    pub knot_id: String,
    pub base_commit: String,
}

/// Record that `knot_id`'s events were skipped as of `base_commit`, unless a
/// row already exists for it. Later skips of the same knot are no-ops so the
/// original base commit is never lost.
pub fn quarantine_knot_if_absent(
    conn: &Connection,
    knot_id: &str,
    base_commit: &str,
) -> Result<()> {
    with_write_retry(|| {
        conn.execute(
            r#"
INSERT INTO knot_sync_quarantine (knot_id, base_commit, quarantined_at)
VALUES (?1, ?2, ?3)
ON CONFLICT(knot_id) DO NOTHING
"#,
            params![knot_id, base_commit, now_utc_rfc3339()],
        )?;
        Ok(())
    })?;
    Ok(())
}

/// All currently quarantined knots, ordered by id for deterministic replay.
pub fn list_quarantined_knots(conn: &Connection) -> Result<Vec<QuarantinedKnot>> {
    let mut stmt =
        conn.prepare("SELECT knot_id, base_commit FROM knot_sync_quarantine ORDER BY knot_id")?;
    let rows = stmt.query_map([], |row| {
        Ok(QuarantinedKnot {
            knot_id: row.get(0)?,
            base_commit: row.get(1)?,
        })
    })?;
    rows.collect()
}

/// Drop a knot's quarantine row once its held-back events have replayed.
pub fn remove_quarantine(conn: &Connection, knot_id: &str) -> Result<()> {
    with_write_retry(|| {
        conn.execute(
            "DELETE FROM knot_sync_quarantine WHERE knot_id = ?1",
            params![knot_id],
        )?;
        Ok(())
    })?;
    Ok(())
}

/// Knot ids this machine currently holds a local lease on: the lease knots
/// themselves, plus any knot bound to one of them via `lease_id`. A lease
/// owned by another machine never contributes to this set -- see
/// `LOCAL_LEASE_PREDICATE` -- so a remote agent's in-flight work never
/// causes a local skip.
pub fn local_leased_knot_ids(conn: &Connection, machine_id: &str) -> Result<HashSet<String>> {
    let sql = format!(
        "SELECT id FROM knot_hot{active}{local}\n\
         UNION\n\
         SELECT id FROM knot_hot WHERE lease_id IN (SELECT id FROM knot_hot{active}{local})",
        active = ACTIVE_LEASE_PREDICATE,
        local = LOCAL_LEASE_PREDICATE,
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![machine_id], |row| row.get::<_, String>(0))?;
    rows.collect()
}
