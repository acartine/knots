use serde_json::Value;

use crate::db::{EdgeDirection, KnotCacheRecord};
use crate::domain::gate::GateData;
use crate::domain::invariant::Invariant;
use crate::domain::knot_type::KnotType;
use crate::domain::metadata::{normalize_datetime, MetadataEntry, MetadataEntryInput};
use crate::domain::step_history::{derive_phase, StepActorInfo, StepRecord, StepStatus};
use crate::installed_workflows;
use crate::workflow::{normalize_profile_id, ProfileDefinition};
use crate::workflow_runtime;

use super::error::AppError;
use super::types::StateActorMetadata;

pub(crate) fn ensure_parent_dir(path: &str) -> Result<(), AppError> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

pub(crate) fn parse_edge_direction(raw: &str) -> Result<EdgeDirection, AppError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "incoming" | "in" => Ok(EdgeDirection::Incoming),
        "outgoing" | "out" => Ok(EdgeDirection::Outgoing),
        "both" | "all" => Ok(EdgeDirection::Both),
        _ => Err(AppError::InvalidArgument(format!(
            "unsupported edge direction '{}'; use incoming|outgoing|both",
            raw
        ))),
    }
}

pub(crate) fn non_empty(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub(crate) fn canonical_profile_id(raw: &str, workflow_id: &str) -> String {
    let trimmed = raw.trim();
    let unqualified =
        if workflow_id != installed_workflows::COMPATIBILITY_WORKFLOW_ID && trimmed.contains('/') {
            trimmed
        } else {
            trimmed.rsplit('/').next().unwrap_or(trimmed)
        };
    normalize_profile_id(unqualified).unwrap_or_else(|| unqualified.to_ascii_lowercase())
}

pub(crate) fn normalize_state_input(raw: &str) -> Result<String, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidArgument("state is required".to_string()));
    }
    let normalized = trimmed.to_ascii_lowercase().replace('-', "_");
    Ok(match normalized.as_str() {
        "idea" => "ready_for_planning".to_string(),
        "work_item" => "ready_for_implementation".to_string(),
        "implementing" => "implementation".to_string(),
        "implemented" => "ready_for_implementation_review".to_string(),
        "reviewing" => "implementation_review".to_string(),
        "rejected" | "refining" => "ready_for_implementation".to_string(),
        "approved" => "ready_for_shipment".to_string(),
        _ => normalized,
    })
}

pub(crate) fn normalize_tag(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

pub(crate) fn next_deferred_from_state(
    current: &KnotCacheRecord,
    next_state: &str,
) -> Option<String> {
    if next_state == "deferred" && current.state != "deferred" {
        Some(current.state.clone())
    } else if current.state == "deferred" && next_state != "deferred" {
        None
    } else {
        current.deferred_from_state.clone()
    }
}

pub(crate) fn next_blocked_from_state(
    profile: &ProfileDefinition,
    current: &KnotCacheRecord,
    next_state: &str,
) -> Option<String> {
    if next_state == "blocked" && current.state != "blocked" {
        blocked_resume_state(profile, current)
    } else if current.state == "blocked" && next_state != "blocked" {
        None
    } else {
        current.blocked_from_state.clone()
    }
}

fn blocked_resume_state(profile: &ProfileDefinition, current: &KnotCacheRecord) -> Option<String> {
    if profile.is_queue_state(&current.state) {
        return Some(current.state.clone());
    }
    let current_idx = profile
        .states
        .iter()
        .position(|state| state == &current.state)?;
    profile.states[..current_idx]
        .iter()
        .rposition(|state| profile.is_queue_state(state))
        .map(|idx| profile.states[idx].clone())
}

pub(crate) fn require_state_for_knot_type(
    knot_type: KnotType,
    profile: &ProfileDefinition,
    state: &str,
) -> Result<(), AppError> {
    match knot_type {
        KnotType::Work => Ok(profile.require_state(state)?),
        KnotType::Gate => {
            if matches!(
                state,
                workflow_runtime::READY_TO_EVALUATE
                    | workflow_runtime::EVALUATING
                    | "shipped"
                    | "abandoned"
            ) {
                Ok(())
            } else {
                Err(AppError::InvalidArgument(format!(
                    "state '{}' is not valid for gate knots",
                    state
                )))
            }
        }
        KnotType::Lease => {
            if matches!(
                state,
                workflow_runtime::LEASE_READY
                    | workflow_runtime::LEASE_ACTIVE
                    | workflow_runtime::LEASE_TERMINATED
            ) {
                Ok(())
            } else {
                Err(AppError::InvalidArgument(format!(
                    "state '{}' is not valid for lease knots",
                    state
                )))
            }
        }
    }
}

pub(crate) fn require_gate_metadata_scope(knot_type: KnotType) -> Result<(), AppError> {
    if knot_type == KnotType::Gate {
        Ok(())
    } else {
        Err(AppError::InvalidArgument(
            "gate owner/failure mode fields require knot type 'gate'".to_string(),
        ))
    }
}

pub(crate) struct KnotHeadData<'a> {
    pub knot_id: &'a str,
    pub title: &'a str,
    pub state: &'a str,
    pub workflow_id: &'a str,
    pub profile_id: &'a str,
    pub updated_at: &'a str,
    pub terminal: bool,
    pub deferred_from_state: Option<&'a str>,
    pub blocked_from_state: Option<&'a str>,
    pub invariants: &'a [Invariant],
    pub knot_type: KnotType,
    pub gate_data: &'a GateData,
}

pub(crate) fn build_knot_head_data(head: KnotHeadData<'_>) -> Value {
    let mut payload = serde_json::Map::new();
    payload.insert(
        "knot_id".to_string(),
        Value::String(head.knot_id.to_string()),
    );
    payload.insert("title".to_string(), Value::String(head.title.to_string()));
    payload.insert("state".to_string(), Value::String(head.state.to_string()));
    payload.insert(
        "profile_id".to_string(),
        Value::String(head.profile_id.to_string()),
    );
    payload.insert(
        "workflow_id".to_string(),
        Value::String(head.workflow_id.to_string()),
    );
    payload.insert(
        "updated_at".to_string(),
        Value::String(head.updated_at.to_string()),
    );
    payload.insert("terminal".to_string(), Value::Bool(head.terminal));
    payload.insert(
        "type".to_string(),
        Value::String(head.knot_type.as_str().to_string()),
    );
    payload.insert(
        "invariants".to_string(),
        serde_json::to_value(head.invariants).expect("invariants should serialize"),
    );
    payload.insert(
        "gate".to_string(),
        serde_json::to_value(head.gate_data).expect("gate data should serialize"),
    );
    insert_optional_string(
        &mut payload,
        "deferred_from_state",
        head.deferred_from_state,
    );
    insert_optional_string(&mut payload, "blocked_from_state", head.blocked_from_state);
    Value::Object(payload)
}

fn insert_optional_string(
    payload: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Option<&str>,
) {
    if let Some(value) = value {
        payload.insert(key.to_string(), Value::String(value.to_string()));
    } else {
        payload.insert(key.to_string(), Value::Null);
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct StateCascadeMetadata<'a> {
    pub root_id: &'a str,
}

pub(crate) struct StateEventParams<'a> {
    pub from: &'a str,
    pub to: &'a str,
    pub workflow_id: &'a str,
    pub profile_id: &'a str,
    pub force: bool,
    pub deferred_from_state: Option<&'a str>,
    pub blocked_from_state: Option<&'a str>,
    pub state_actor: &'a StateActorMetadata,
    pub cascade: Option<StateCascadeMetadata<'a>>,
}

pub(crate) fn build_state_event_data(params: &StateEventParams<'_>) -> Result<Value, AppError> {
    let mut payload = serde_json::Map::new();
    payload.insert("from".to_string(), Value::String(params.from.to_string()));
    payload.insert("to".to_string(), Value::String(params.to.to_string()));
    payload.insert(
        "workflow_id".to_string(),
        Value::String(params.workflow_id.to_string()),
    );
    payload.insert(
        "profile_id".to_string(),
        Value::String(params.profile_id.to_string()),
    );
    payload.insert("force".to_string(), Value::Bool(params.force));
    if let Some(value) = params.deferred_from_state {
        payload.insert(
            "deferred_from_state".to_string(),
            Value::String(value.to_string()),
        );
    }
    if let Some(value) = params.blocked_from_state {
        payload.insert(
            "blocked_from_state".to_string(),
            Value::String(value.to_string()),
        );
    }
    if let Some(cascade) = params.cascade {
        payload.insert("cascade_approved".to_string(), Value::Bool(true));
        payload.insert(
            "cascade_root_id".to_string(),
            Value::String(cascade.root_id.to_string()),
        );
    }
    append_state_actor_metadata(&mut payload, params.state_actor)?;
    Ok(Value::Object(payload))
}

pub(crate) fn append_state_actor_metadata(
    payload: &mut serde_json::Map<String, Value>,
    state_actor: &StateActorMetadata,
) -> Result<(), AppError> {
    if let Some(raw_kind) = state_actor.actor_kind.as_deref().and_then(non_empty) {
        let kind = raw_kind.to_ascii_lowercase();
        if kind != "human" && kind != "agent" {
            return Err(AppError::InvalidArgument(
                "--actor-kind must be one of: human, agent".to_string(),
            ));
        }
        payload.insert("actor_kind".to_string(), Value::String(kind));
    }
    if let Some(agent_name) = state_actor.agent_name.as_deref().and_then(non_empty) {
        payload.insert("agent_name".to_string(), Value::String(agent_name));
    }
    if let Some(agent_model) = state_actor.agent_model.as_deref().and_then(non_empty) {
        payload.insert("agent_model".to_string(), Value::String(agent_model));
    }
    if let Some(agent_version) = state_actor.agent_version.as_deref().and_then(non_empty) {
        payload.insert("agent_version".to_string(), Value::String(agent_version));
    }
    Ok(())
}

pub(crate) fn metadata_entry_from_input(
    input: MetadataEntryInput,
    fallback_datetime: &str,
) -> Result<MetadataEntry, AppError> {
    if input.content.trim().is_empty() {
        return Err(AppError::InvalidArgument(
            "metadata content cannot be empty".to_string(),
        ));
    }
    if let Some(raw) = input.datetime.as_deref() {
        if normalize_datetime(Some(raw)).is_none() {
            return Err(AppError::InvalidArgument(
                "metadata datetime must be RFC3339".to_string(),
            ));
        }
    }
    Ok(MetadataEntry::from_input(input, fallback_datetime))
}

pub(crate) fn ensure_profile_etag(
    current: &KnotCacheRecord,
    expected_profile_etag: Option<&str>,
) -> Result<(), AppError> {
    let Some(expected) = expected_profile_etag else {
        return Ok(());
    };
    let current_etag = current.profile_etag.as_deref().unwrap_or("");
    if current_etag == expected {
        return Ok(());
    }
    Err(AppError::StaleWorkflowHead {
        expected: expected.to_string(),
        current: current_etag.to_string(),
    })
}

fn is_action_state(state: &str) -> bool {
    workflow_runtime::is_action_state(state)
}

pub(crate) fn apply_step_transition(
    existing: &[StepRecord],
    from_state: &str,
    to_state: &str,
    occurred_at: &str,
    actor: &StateActorMetadata,
    lease_id: Option<&str>,
) -> Vec<StepRecord> {
    let mut history: Vec<StepRecord> = existing.to_vec();
    if from_state != to_state {
        for record in &mut history {
            if record.is_active() {
                record.to_state = Some(to_state.to_string());
                record.ended_at = Some(occurred_at.to_string());
                record.status = StepStatus::Completed;
            }
        }
    }
    if is_action_state(to_state) && from_state != to_state {
        let step_actor = StepActorInfo {
            actor_kind: actor.actor_kind.clone(),
            agent_name: actor.agent_name.clone(),
            agent_model: actor.agent_model.clone(),
            agent_version: actor.agent_version.clone(),
            lease_id: lease_id.map(|s| s.to_string()),
            ..Default::default()
        };
        let phase = derive_phase(to_state);
        let record = StepRecord::new_started(to_state, phase, from_state, occurred_at, &step_actor);
        history.push(record);
    }
    history
}

pub fn annotate_step_history(
    existing: &[StepRecord],
    actor: &StepActorInfo,
    occurred_at: &str,
) -> Vec<StepRecord> {
    let mut history: Vec<StepRecord> = existing.to_vec();
    let has_active = history.iter().any(|r| r.is_active());
    if has_active {
        let mut new_record: Option<StepRecord> = None;
        for record in &mut history {
            if record.is_active() {
                record.ended_at = Some(occurred_at.to_string());
                record.status = StepStatus::Completed;
                new_record = Some(StepRecord::new_started(
                    &record.step,
                    &record.phase,
                    &record.from_state,
                    occurred_at,
                    actor,
                ));
            }
        }
        if let Some(new) = new_record {
            history.push(new);
        }
    }
    history
}
