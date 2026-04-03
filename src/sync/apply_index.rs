use std::path::Path;

use rusqlite::Connection;
use serde_json::Value;
use time::OffsetDateTime;

use crate::db;
use crate::tiering::{classify_knot_tier, CacheTier};

use super::apply_helpers::{
    optional_string, parse_gate_data, parse_invariants, MetadataProjection,
};
use super::SyncError;

pub(super) fn resolve_tier(
    conn: &Connection,
    data: &serde_json::Map<String, Value>,
    state: &str,
    updated_at: &str,
) -> Result<CacheTier, SyncError> {
    let hot_window_days = db::get_hot_window_days(conn)?;
    let terminal_flag = data
        .get("terminal")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let now = OffsetDateTime::now_utc();
    if terminal_flag {
        Ok(CacheTier::Cold)
    } else {
        Ok(classify_knot_tier(state, updated_at, hot_window_days, now))
    }
}

pub(super) struct IndexUpsertParams<'a> {
    pub(super) conn: &'a Connection,
    pub(super) data: &'a serde_json::Map<String, Value>,
    pub(super) absolute_path: &'a Path,
    pub(super) knot_id: &'a str,
    pub(super) title: &'a str,
    pub(super) state: &'a str,
    pub(super) updated_at: &'a str,
    pub(super) profile_id: &'a str,
    pub(super) workflow_id: &'a str,
    pub(super) event_id: &'a str,
}

pub(super) fn build_index_upsert(
    params: &IndexUpsertParams<'_>,
) -> Result<MetadataProjection, SyncError> {
    let existing = db::get_knot_hot(params.conn, params.knot_id)?;
    let body = existing.as_ref().and_then(|r| r.body.clone());
    let description = existing.as_ref().and_then(|r| r.description.clone());
    let acceptance = existing.as_ref().and_then(|r| r.acceptance.clone());
    let priority = existing.as_ref().and_then(|r| r.priority);
    let knot_type = existing.as_ref().and_then(|r| r.knot_type.clone());
    let tags = existing
        .as_ref()
        .map(|r| r.tags.clone())
        .unwrap_or_default();
    let notes = existing
        .as_ref()
        .map(|r| r.notes.clone())
        .unwrap_or_default();
    let handoff_capsules = existing
        .as_ref()
        .map(|r| r.handoff_capsules.clone())
        .unwrap_or_default();
    let mut invariants = existing
        .as_ref()
        .map(|r| r.invariants.clone())
        .unwrap_or_default();
    if params.data.contains_key("invariants") {
        invariants = parse_invariants(params.data, params.absolute_path)?;
    }
    let step_history = existing
        .as_ref()
        .map(|r| r.step_history.clone())
        .unwrap_or_default();
    let mut gate_data = existing
        .as_ref()
        .map(|r| r.gate_data.clone())
        .unwrap_or_default();
    if params.data.contains_key("gate") {
        gate_data = parse_gate_data(params.data, params.absolute_path)?;
    }
    let lease_data = existing
        .as_ref()
        .map(|r| r.lease_data.clone())
        .unwrap_or_default();
    let lease_id = existing.as_ref().and_then(|r| r.lease_id.clone());
    let deferred_from_state =
        optional_string(params.data.get("deferred_from_state")).or_else(|| {
            existing
                .as_ref()
                .and_then(|r| r.deferred_from_state.clone())
        });
    let blocked_from_state = optional_string(params.data.get("blocked_from_state"))
        .or_else(|| existing.as_ref().and_then(|r| r.blocked_from_state.clone()));
    let created_at = existing
        .as_ref()
        .and_then(|r| r.created_at.clone())
        .unwrap_or_else(|| params.updated_at.to_string());

    Ok(MetadataProjection {
        title: params.title.to_string(),
        state: params.state.to_string(),
        updated_at: params.updated_at.to_string(),
        body,
        description,
        acceptance,
        priority,
        knot_type,
        tags,
        notes,
        handoff_capsules,
        invariants,
        step_history,
        gate_data,
        lease_data,
        lease_id,
        workflow_id: params.workflow_id.to_string(),
        profile_id: params.profile_id.to_string(),
        profile_etag: Some(params.event_id.to_string()),
        deferred_from_state,
        blocked_from_state,
        created_at: Some(created_at),
    })
}
