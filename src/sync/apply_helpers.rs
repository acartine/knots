use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::db::{self, KnotCacheRecord, UpsertKnotHot};
use crate::domain::gate::GateData;
use crate::domain::invariant::Invariant;
use crate::domain::lease::LeaseData;
use crate::domain::metadata::MetadataEntry;
use crate::domain::step_history::StepRecord;
use crate::installed_workflows;

use super::SyncError;

pub(super) fn current_unix_ms_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}

pub(super) struct MetadataProjection {
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
    pub invariants: Vec<Invariant>,
    pub step_history: Vec<StepRecord>,
    pub gate_data: GateData,
    pub lease_data: LeaseData,
    pub lease_id: Option<String>,
    pub workflow_id: String,
    pub profile_id: String,
    pub profile_etag: Option<String>,
    pub deferred_from_state: Option<String>,
    pub blocked_from_state: Option<String>,
    pub created_at: Option<String>,
}

impl MetadataProjection {
    pub fn from_existing(existing: &KnotCacheRecord) -> Self {
        Self {
            title: existing.title.clone(),
            state: existing.state.clone(),
            updated_at: existing.updated_at.clone(),
            body: existing.body.clone(),
            description: existing.description.clone(),
            acceptance: existing.acceptance.clone(),
            priority: existing.priority,
            knot_type: existing.knot_type.clone(),
            tags: existing.tags.clone(),
            notes: existing.notes.clone(),
            handoff_capsules: existing.handoff_capsules.clone(),
            invariants: existing.invariants.clone(),
            step_history: existing.step_history.clone(),
            gate_data: existing.gate_data.clone(),
            lease_data: existing.lease_data.clone(),
            lease_id: existing.lease_id.clone(),
            workflow_id: existing.workflow_id.clone(),
            profile_id: existing.profile_id.clone(),
            profile_etag: existing.profile_etag.clone(),
            deferred_from_state: existing.deferred_from_state.clone(),
            blocked_from_state: existing.blocked_from_state.clone(),
            created_at: existing.created_at.clone(),
        }
    }

    pub fn upsert(&self, conn: &Connection, id: &str) -> Result<(), SyncError> {
        db::upsert_knot_hot(
            conn,
            &UpsertKnotHot {
                id,
                title: &self.title,
                state: &self.state,
                updated_at: &self.updated_at,
                body: self.body.as_deref(),
                description: self.description.as_deref(),
                acceptance: self.acceptance.as_deref(),
                priority: self.priority,
                knot_type: self.knot_type.as_deref(),
                tags: &self.tags,
                notes: &self.notes,
                handoff_capsules: &self.handoff_capsules,
                invariants: &self.invariants,
                step_history: &self.step_history,
                gate_data: &self.gate_data,
                lease_data: &self.lease_data,
                lease_id: self.lease_id.as_deref(),
                workflow_id: &self.workflow_id,
                profile_id: &self.profile_id,
                profile_etag: self.profile_etag.as_deref(),
                deferred_from_state: self.deferred_from_state.as_deref(),
                blocked_from_state: self.blocked_from_state.as_deref(),
                created_at: self.created_at.as_deref(),
            },
        )?;
        Ok(())
    }
}

pub(super) fn read_json_file<T>(path: &Path) -> Result<T, SyncError>
where
    T: DeserializeOwned,
{
    let bytes = std::fs::read(path)?;
    serde_json::from_slice(&bytes)
        .map_err(|err| invalid_event(path, &format!("invalid JSON payload: {}", err)))
}

pub(super) fn required_string(
    object: &Map<String, Value>,
    key: &str,
    path: &Path,
) -> Result<String, SyncError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(|value| value.to_string())
        .ok_or_else(|| invalid_event(path, &format!("missing '{}' string field", key)))
}

pub(super) fn required_profile_id(
    object: &Map<String, Value>,
    path: &Path,
) -> Result<String, SyncError> {
    if let Some(value) = object.get("profile_id").and_then(Value::as_str) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    Err(invalid_event(path, "missing 'profile_id' string field"))
}

pub(super) struct ResolvedWorkflowId {
    pub id: String,
    pub converted_from: Option<String>,
}

pub(super) fn required_workflow_id(
    object: &Map<String, Value>,
    path: &Path,
) -> Result<ResolvedWorkflowId, SyncError> {
    if let Some(value) = object.get("workflow_id").and_then(Value::as_str) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            let normalized = installed_workflows::normalize_workflow_id(trimmed);
            return Ok(match normalized.as_str() {
                "compatibility" | "knots_sdlc" => ResolvedWorkflowId {
                    id: "work_sdlc".to_string(),
                    converted_from: Some(normalized),
                },
                _ => ResolvedWorkflowId {
                    id: normalized,
                    converted_from: None,
                },
            });
        }
    }
    Err(invalid_event(path, "missing 'workflow_id' string field"))
}

pub(super) fn optional_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .and_then(|raw| {
            if raw.is_empty() {
                None
            } else {
                Some(raw.to_string())
            }
        })
}

pub(super) fn optional_i64(value: Option<&Value>) -> Option<i64> {
    value.and_then(Value::as_i64)
}

pub(super) fn parse_metadata_entry(
    object: &Map<String, Value>,
    path: &Path,
) -> Result<MetadataEntry, SyncError> {
    let entry_id = required_string(object, "entry_id", path)?;
    let content = required_string(object, "content", path)?;
    let username = required_string(object, "username", path)?;
    let datetime = required_string(object, "datetime", path)?;
    let agentname = required_string(object, "agentname", path)?;
    let model = required_string(object, "model", path)?;
    let version = required_string(object, "version", path)?;
    Ok(MetadataEntry {
        entry_id,
        content,
        username,
        datetime,
        agentname,
        model,
        version,
    })
}

pub(super) fn parse_invariants(
    object: &Map<String, Value>,
    path: &Path,
) -> Result<Vec<Invariant>, SyncError> {
    let raw = object
        .get("invariants")
        .ok_or_else(|| invalid_event(path, "missing 'invariants' array field"))?;
    serde_json::from_value(raw.clone())
        .map_err(|err| invalid_event(path, &format!("invalid 'invariants' payload: {}", err)))
}

pub(super) fn parse_gate_data(
    object: &Map<String, Value>,
    path: &Path,
) -> Result<GateData, SyncError> {
    let raw = object
        .get("gate")
        .ok_or_else(|| invalid_event(path, "missing 'gate' object field"))?;
    serde_json::from_value(raw.clone())
        .map_err(|err| invalid_event(path, &format!("invalid 'gate' payload: {}", err)))
}

pub(super) fn parse_lease_data(
    object: &Map<String, Value>,
    path: &Path,
) -> Result<LeaseData, SyncError> {
    let raw = object
        .get("lease_data")
        .ok_or_else(|| invalid_event(path, "missing 'lease_data' field"))?;
    serde_json::from_value(raw.clone())
        .map_err(|err| invalid_event(path, &format!("invalid 'lease_data' payload: {}", err)))
}

pub(super) fn invalid_event(path: &Path, message: &str) -> SyncError {
    SyncError::InvalidEvent {
        path: path.to_path_buf(),
        message: message.to_string(),
    }
}

pub(super) fn is_stale_precondition(
    conn: &Connection,
    knot_id: &str,
    precondition: Option<&crate::events::WorkflowPrecondition>,
) -> Result<bool, SyncError> {
    let Some(precondition) = precondition else {
        return Ok(false);
    };
    let current = db::get_knot_hot(conn, knot_id)?
        .and_then(|record| record.profile_etag)
        .unwrap_or_default();
    Ok(current != precondition.profile_etag)
}

pub(super) struct IndexUpsertParams<'a> {
    pub conn: &'a Connection,
    pub data: &'a serde_json::Map<String, serde_json::Value>,
    pub absolute_path: &'a std::path::Path,
    pub knot_id: &'a str,
    pub title: &'a str,
    pub state: &'a str,
    pub updated_at: &'a str,
    pub profile_id: &'a str,
    pub workflow_id: &'a str,
    pub event_id: &'a str,
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
