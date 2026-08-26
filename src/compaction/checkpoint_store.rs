use std::collections::{BTreeMap, BTreeSet, HashSet};

use rusqlite::{params, Connection, OptionalExtension};

use crate::db::{self, EdgeRecord, KnotCacheRecord, UpsertKnotHot};

use super::{CheckpointError, CheckpointSummary, PreparedCheckpoint, WriterHead};

pub(super) fn install<F>(
    conn: &Connection,
    checkpoint: &PreparedCheckpoint,
    locally_leased: &HashSet<String>,
    replay_delta: F,
) -> Result<CheckpointSummary, CheckpointError>
where
    F: FnOnce(&Connection) -> Result<(), String>,
{
    let tx = conn.unchecked_transaction()?;
    validate_predecessor(&tx, checkpoint)?;
    let existing_quarantine = existing_generation_quarantine(&tx)?;
    let quarantined = quarantine_ids(&tx, locally_leased)?;
    let preserved = preserved_hot(&tx, &quarantined)?;
    let preserved_edges = preserved_edges(&tx, &quarantined)?;
    replace_projections(&tx, checkpoint, &quarantined, &preserved, &preserved_edges)?;
    replace_checkpoint_metadata(&tx, checkpoint, &quarantined, &existing_quarantine)?;
    replay_delta(&tx).map_err(CheckpointError::Replay)?;
    tx.commit()?;
    Ok(CheckpointSummary {
        generation_id: checkpoint.manifest.generation_id.clone(),
        control_epoch: checkpoint.control.epoch,
        hot: checkpoint.hot.len(),
        warm: checkpoint.warm.len(),
        cold: checkpoint.cold.len(),
        edges: checkpoint.edges.len(),
        quarantined: quarantined.len(),
    })
}

type QuarantineBaseline = (String, String, String);

fn validate_predecessor(
    conn: &Connection,
    checkpoint: &PreparedCheckpoint,
) -> Result<(), CheckpointError> {
    let current = conn
        .query_row(
            "SELECT generation_id, control_epoch, control_head FROM v2_checkpoint WHERE singleton = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    let expected_generation = checkpoint.manifest.predecessor_generation.as_deref();
    let expected_epoch = checkpoint.manifest.predecessor_control_epoch;
    let expected_head = checkpoint.control.previous_control_head.as_deref();
    let matches = match current {
        None => expected_generation.is_none() && expected_epoch == 0 && expected_head.is_none(),
        Some((generation, epoch, head)) => {
            expected_generation == Some(generation.as_str())
                && u64::try_from(epoch).ok() == Some(expected_epoch)
                && expected_head == Some(head.as_str())
        }
    };
    if matches {
        Ok(())
    } else {
        Err(CheckpointError::Control(
            "checkpoint predecessor no longer matches local control state".to_string(),
        ))
    }
}

fn existing_generation_quarantine(
    conn: &Connection,
) -> Result<BTreeMap<String, QuarantineBaseline>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT knot_id, base_generation_id, writer_vector_json, quarantined_at \
         FROM v2_generation_quarantine ORDER BY knot_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            (row.get(1)?, row.get(2)?, row.get(3)?),
        ))
    })?;
    rows.collect()
}

pub(crate) fn drain_generation_quarantine<F>(
    conn: &Connection,
    knot_id: &str,
    replay: F,
) -> Result<bool, CheckpointError>
where
    F: FnOnce(&Connection) -> Result<(), String>,
{
    let tx = conn.unchecked_transaction()?;
    let exists: i64 = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM v2_generation_quarantine WHERE knot_id = ?1)",
        params![knot_id],
        |row| row.get(0),
    )?;
    if exists == 0 {
        tx.commit()?;
        return Ok(false);
    }
    replay(&tx).map_err(CheckpointError::Replay)?;
    tx.execute(
        "DELETE FROM v2_generation_quarantine WHERE knot_id = ?1",
        params![knot_id],
    )?;
    tx.commit()?;
    Ok(true)
}

fn quarantine_ids(
    conn: &Connection,
    locally_leased: &HashSet<String>,
) -> Result<BTreeSet<String>, rusqlite::Error> {
    let mut ids = locally_leased.iter().cloned().collect::<BTreeSet<_>>();
    let mut stmt = conn.prepare("SELECT knot_id FROM knot_sync_quarantine")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    for row in rows {
        ids.insert(row?);
    }
    let mut stmt = conn.prepare("SELECT knot_id FROM v2_generation_quarantine")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    for row in rows {
        ids.insert(row?);
    }
    Ok(ids)
}

fn preserved_hot(
    conn: &Connection,
    quarantined: &BTreeSet<String>,
) -> Result<Vec<KnotCacheRecord>, rusqlite::Error> {
    quarantined
        .iter()
        .map(|id| db::get_knot_hot(conn, id))
        .filter_map(|result| match result {
            Ok(Some(record)) => Some(Ok(record)),
            Ok(None) => None,
            Err(error) => Some(Err(error)),
        })
        .collect()
}

fn preserved_edges(
    conn: &Connection,
    quarantined: &BTreeSet<String>,
) -> Result<Vec<EdgeRecord>, rusqlite::Error> {
    let mut unique = BTreeSet::new();
    for id in quarantined {
        for edge in db::list_edges(conn, id, db::EdgeDirection::Both)? {
            unique.insert((edge.src, edge.kind, edge.dst));
        }
    }
    Ok(unique
        .into_iter()
        .map(|(src, kind, dst)| EdgeRecord { src, kind, dst })
        .collect())
}

fn replace_projections(
    conn: &Connection,
    checkpoint: &PreparedCheckpoint,
    quarantined: &BTreeSet<String>,
    preserved: &[KnotCacheRecord],
    preserved_edges: &[EdgeRecord],
) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "DELETE FROM edge; DELETE FROM knot_hot; DELETE FROM knot_warm;\
         DELETE FROM cold_catalog; DELETE FROM review_stats; DELETE FROM v2_projection_aux;",
    )?;
    for record in checkpoint
        .hot
        .iter()
        .filter(|record| !quarantined.contains(&record.id))
        .chain(preserved)
    {
        write_hot(conn, record)?;
    }
    for record in &checkpoint.warm {
        if !quarantined.contains(&record.id) {
            db::upsert_knot_warm(conn, &record.id, &record.title)?;
        }
    }
    for record in &checkpoint.cold {
        if !quarantined.contains(&record.id) {
            db::upsert_cold_catalog(
                conn,
                &record.id,
                &record.title,
                &record.state,
                &record.updated_at,
            )?;
        }
    }
    for edge in &checkpoint.edges {
        if !quarantined.contains(&edge.src) && !quarantined.contains(&edge.dst) {
            db::insert_edge(conn, &edge.src, &edge.kind, &edge.dst)?;
        }
    }
    for edge in preserved_edges {
        db::insert_edge(conn, &edge.src, &edge.kind, &edge.dst)?;
    }
    for (kind, ordinal, payload) in &checkpoint.auxiliary {
        conn.execute(
            "INSERT INTO v2_projection_aux(kind, ordinal, payload_json) VALUES (?1, ?2, ?3)",
            params![kind, *ordinal as i64, payload],
        )?;
    }
    Ok(())
}

fn replace_checkpoint_metadata(
    conn: &Connection,
    checkpoint: &PreparedCheckpoint,
    quarantined: &BTreeSet<String>,
    existing_quarantine: &BTreeMap<String, QuarantineBaseline>,
) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "DELETE FROM v2_checkpoint; DELETE FROM v2_checkpoint_writer_head;\
         DELETE FROM v2_checkpoint_event; DELETE FROM v2_generation_quarantine;",
    )?;
    conn.execute(
        "INSERT INTO v2_checkpoint VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            checkpoint.manifest.generation_id,
            sqlite_u64(checkpoint.control.epoch)?,
            checkpoint.control_head,
            checkpoint.control.active_generation_commit,
            checkpoint.manifest.source.cutoff_commit,
            crate::db::now_utc_rfc3339(),
        ],
    )?;
    for head in &checkpoint.manifest.writer_heads {
        insert_writer_head(conn, head)?;
    }
    for pack in &checkpoint.manifest.packs {
        for event in &pack.events {
            conn.execute(
                "INSERT INTO v2_checkpoint_event VALUES (?1, ?2, ?3)",
                params![event.event_id, event.content_sha256, pack.pack_id],
            )?;
        }
    }
    write_quarantine(conn, checkpoint, quarantined, existing_quarantine)?;
    conn.execute("DELETE FROM knot_sync_quarantine", [])?;
    db::set_meta(conn, "v2_generation_id", &checkpoint.manifest.generation_id)?;
    db::set_meta(
        conn,
        "v2_control_epoch",
        &checkpoint.control.epoch.to_string(),
    )?;
    for key in ["last_index_head_commit", "last_full_head_commit"] {
        db::set_meta(conn, key, &checkpoint.manifest.source.cutoff_commit)?;
    }
    Ok(())
}

fn write_quarantine(
    conn: &Connection,
    checkpoint: &PreparedCheckpoint,
    quarantined: &BTreeSet<String>,
    existing: &BTreeMap<String, QuarantineBaseline>,
) -> Result<(), rusqlite::Error> {
    let vector = serde_json::to_string(&checkpoint.manifest.writer_heads)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    for knot_id in quarantined {
        if let Some((generation, existing_vector, quarantined_at)) = existing.get(knot_id) {
            conn.execute(
                "INSERT INTO v2_generation_quarantine VALUES (?1, ?2, ?3, ?4)",
                params![knot_id, generation, existing_vector, quarantined_at],
            )?;
            continue;
        }
        conn.execute(
            "INSERT INTO v2_generation_quarantine VALUES (?1, ?2, ?3, ?4)",
            params![
                knot_id,
                checkpoint.manifest.generation_id,
                vector,
                crate::db::now_utc_rfc3339(),
            ],
        )?;
    }
    Ok(())
}

fn insert_writer_head(conn: &Connection, head: &WriterHead) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO v2_checkpoint_writer_head VALUES (?1, ?2, ?3, ?4)",
        params![
            head.writer_id,
            head.inbox_ref,
            head.commit,
            sqlite_u64(head.sequence)?
        ],
    )?;
    Ok(())
}

fn sqlite_u64(value: u64) -> Result<i64, rusqlite::Error> {
    i64::try_from(value).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

fn write_hot(conn: &Connection, record: &KnotCacheRecord) -> Result<(), rusqlite::Error> {
    db::upsert_knot_hot(conn, &upsert(record))?;
    db::update_knot_scope_data(conn, &record.id, &record.scope_data)
}

fn upsert(record: &KnotCacheRecord) -> UpsertKnotHot<'_> {
    UpsertKnotHot {
        id: &record.id,
        title: &record.title,
        state: &record.state,
        updated_at: &record.updated_at,
        body: record.body.as_deref(),
        description: record.description.as_deref(),
        acceptance: record.acceptance.as_deref(),
        priority: record.priority,
        knot_type: record.knot_type.as_deref(),
        tags: &record.tags,
        notes: &record.notes,
        handoff_capsules: &record.handoff_capsules,
        invariants: &record.invariants,
        verification_steps: &record.verification_steps,
        step_history: &record.step_history,
        gate_data: &record.gate_data,
        lease_data: &record.lease_data,
        execution_plan_data: &record.execution_plan_data,
        lease_id: record.lease_id.as_deref(),
        lease_expiry_ts: record.lease_expiry_ts,
        workflow_id: &record.workflow_id,
        profile_id: &record.profile_id,
        profile_etag: record.profile_etag.as_deref(),
        deferred_from_state: record.deferred_from_state.as_deref(),
        blocked_from_state: record.blocked_from_state.as_deref(),
        created_at: record.created_at.as_deref(),
    }
}
