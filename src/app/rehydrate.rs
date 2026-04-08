use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::domain::gate::GateData;
use crate::domain::invariant::Invariant;
use crate::domain::knot_type::{parse_knot_type, KnotType};
use crate::domain::lease::LeaseData;
use crate::domain::metadata::MetadataEntry;
use crate::domain::step_history::StepRecord;
use crate::events::{FullEvent, IndexEvent, IndexEventKind};
use crate::installed_workflows;
use crate::workflow::normalize_profile_id;

use super::error::AppError;

pub(crate) mod apply_event;

pub(crate) struct RehydrateProjection {
    pub title: String,
    pub state: String,
    pub updated_at: String,
    pub body: Option<String>,
    pub description: Option<String>,
    pub acceptance: Option<String>,
    pub priority: Option<i64>,
    pub knot_type: KnotType,
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

pub(crate) fn rehydrate_from_events(
    store_root: &Path,
    knot_id: &str,
    title: String,
    state: String,
    updated_at: String,
) -> Result<RehydrateProjection, AppError> {
    let mut projection = new_projection(title, state, updated_at);
    apply_full_events(store_root, knot_id, &mut projection)?;
    apply_index_events(store_root, knot_id, &mut projection)?;
    finalize_projection(&mut projection, knot_id)?;
    Ok(projection)
}

fn new_projection(title: String, state: String, updated_at: String) -> RehydrateProjection {
    RehydrateProjection {
        title,
        state,
        updated_at: updated_at.clone(),
        body: None,
        description: None,
        acceptance: None,
        priority: None,
        knot_type: KnotType::default(),
        tags: Vec::new(),
        notes: Vec::new(),
        handoff_capsules: Vec::new(),
        invariants: Vec::new(),
        step_history: Vec::new(),
        gate_data: GateData::default(),
        lease_data: LeaseData::default(),
        lease_id: None,
        workflow_id: String::new(),
        profile_id: String::new(),
        profile_etag: None,
        deferred_from_state: None,
        blocked_from_state: None,
        created_at: Some(updated_at),
    }
}

fn collect_json_paths(root: &Path) -> Result<Vec<std::path::PathBuf>, AppError> {
    let mut stack = vec![root.to_path_buf()];
    let mut paths = Vec::new();
    while let Some(dir) = stack.pop() {
        if !dir.exists() {
            continue;
        }
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "json") {
                paths.push(path);
            }
        }
    }
    paths.sort();
    Ok(paths)
}

fn apply_full_events(
    store_root: &Path,
    knot_id: &str,
    projection: &mut RehydrateProjection,
) -> Result<(), AppError> {
    let event_root = resolve_subdir(store_root, "events");
    let full_paths = collect_json_paths(&event_root)?;
    for path in full_paths {
        let bytes = fs::read(&path)?;
        let event: FullEvent = serde_json::from_slice(&bytes).map_err(|err| {
            AppError::InvalidArgument(format!(
                "invalid rehydrate event '{}': {}",
                path.display(),
                err
            ))
        })?;
        if event.knot_id != knot_id {
            continue;
        }
        apply_event::apply_rehydrate_event(projection, &event);
    }
    Ok(())
}

fn apply_index_events(
    store_root: &Path,
    knot_id: &str,
    projection: &mut RehydrateProjection,
) -> Result<(), AppError> {
    let index_root = resolve_subdir(store_root, "index");
    let idx_paths = collect_json_paths(&index_root)?;
    for path in idx_paths {
        let bytes = fs::read(&path)?;
        let event: IndexEvent = serde_json::from_slice(&bytes).map_err(|err| {
            AppError::InvalidArgument(format!(
                "invalid rehydrate index '{}': {}",
                path.display(),
                err
            ))
        })?;
        if event.event_type != IndexEventKind::KnotHead.as_str() {
            continue;
        }
        let Some(data) = event.data.as_object() else {
            continue;
        };
        if data.get("knot_id").and_then(Value::as_str) != Some(knot_id) {
            continue;
        }
        apply_index_head(data, &event.event_id, projection);
    }
    Ok(())
}

fn apply_index_head(
    data: &serde_json::Map<String, Value>,
    event_id: &str,
    projection: &mut RehydrateProjection,
) {
    if let Some(title) = data.get("title").and_then(Value::as_str) {
        projection.title = title.to_string();
    }
    if let Some(state) = data.get("state").and_then(Value::as_str) {
        projection.state = state.to_string();
    }
    if let Some(updated_at) = data.get("updated_at").and_then(Value::as_str) {
        projection.updated_at = updated_at.to_string();
    }
    if let Some(raw_type) = data.get("type").and_then(Value::as_str) {
        projection.knot_type = parse_knot_type(Some(raw_type));
    }
    if let Some(raw_wf) = data.get("workflow_id").and_then(Value::as_str) {
        projection.workflow_id = installed_workflows::normalize_workflow_id(raw_wf);
    }
    let raw_profile = data
        .get("profile_id")
        .and_then(Value::as_str)
        .or_else(|| data.get("workflow_id").and_then(Value::as_str));
    if let Some(raw) = raw_profile {
        if let Some(pid) = normalize_profile_id(raw) {
            projection.profile_id = pid;
        }
    }
    projection.deferred_from_state = data
        .get("deferred_from_state")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    projection.blocked_from_state = data
        .get("blocked_from_state")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    if data.contains_key("invariants") {
        projection.invariants = parse_invariants_value(data.get("invariants"));
    }
    if data.contains_key("gate") {
        projection.gate_data = parse_gate_data_value(data.get("gate"));
    }
    projection.profile_etag = Some(event_id.to_string());
}

fn finalize_projection(
    projection: &mut RehydrateProjection,
    knot_id: &str,
) -> Result<(), AppError> {
    if projection.workflow_id.trim().is_empty() {
        projection.workflow_id =
            installed_workflows::builtin_workflow_id_for_knot_type(KnotType::Work);
    }
    if projection.profile_id.trim().is_empty() {
        if !projection.workflow_id.trim().is_empty() {
            projection.profile_id = projection.workflow_id.clone();
        } else {
            return Err(AppError::InvalidArgument(format!(
                "rehydrate events for '{}' are missing profile_id",
                knot_id
            )));
        }
    }
    Ok(())
}

fn resolve_subdir(store_root: &Path, name: &str) -> std::path::PathBuf {
    let nested = store_root.join(".knots");
    if nested.exists() {
        nested.join(name)
    } else {
        store_root.join(name)
    }
}

pub(crate) fn parse_invariants_value(value: Option<&Value>) -> Vec<Invariant> {
    let Some(value) = value.cloned() else {
        return Vec::new();
    };
    serde_json::from_value(value).unwrap_or_default()
}

pub(crate) fn parse_gate_data_value(value: Option<&Value>) -> GateData {
    let Some(value) = value.cloned() else {
        return GateData::default();
    };
    serde_json::from_value(value).unwrap_or_default()
}

pub(crate) fn parse_metadata_entry_for_rehydrate(
    data: &serde_json::Map<String, Value>,
) -> Option<MetadataEntry> {
    let entry_id = data.get("entry_id")?.as_str()?.to_string();
    let content = data.get("content")?.as_str()?.to_string();
    let username = data.get("username")?.as_str()?.to_string();
    let datetime = data.get("datetime")?.as_str()?.to_string();
    let agentname = data.get("agentname")?.as_str()?.to_string();
    let model = data.get("model")?.as_str()?.to_string();
    let version = data.get("version")?.as_str()?.to_string();
    Some(MetadataEntry {
        entry_id,
        content,
        username,
        datetime,
        agentname,
        model,
        version,
    })
}
