use serde_json::Value;

use crate::domain::knot_type::parse_knot_type;
use crate::events::FullEvent;
use crate::workflow::normalize_profile_id;

use super::{
    parse_gate_data_value, parse_invariants_value, parse_metadata_entry_for_rehydrate,
    RehydrateProjection,
};

pub(crate) fn apply_rehydrate_event(projection: &mut RehydrateProjection, event: &FullEvent) {
    let Some(data) = event.data.as_object() else {
        return;
    };
    match event.event_type.as_str() {
        "knot.created" => apply_created(projection, data, event),
        "knot.title_set" => apply_title_set(projection, data, event),
        "knot.state_set" => apply_state_set(projection, data, event),
        "knot.profile_set" => {
            apply_profile_set(projection, data, event);
        }
        "knot.description_set" => {
            apply_description_set(projection, data, event);
        }
        "knot.acceptance_set" => {
            apply_acceptance_set(projection, data, event);
        }
        "knot.priority_set" => {
            apply_priority_set(projection, data, event);
        }
        "knot.type_set" => apply_type_set(projection, data, event),
        "knot.gate_data_set" => {
            apply_gate_data_set(projection, data, event);
        }
        "knot.tag_add" => apply_tag_add(projection, data),
        "knot.tag_remove" => apply_tag_remove(projection, data),
        "knot.note_added" => apply_note_added(projection, data),
        "knot.handoff_capsule_added" => {
            apply_handoff_capsule_added(projection, data);
        }
        "knot.invariants_set" => {
            apply_invariants_set(projection, data, event);
        }
        _ => {}
    }
}

fn apply_created(
    p: &mut RehydrateProjection,
    data: &serde_json::Map<String, Value>,
    event: &FullEvent,
) {
    if let Some(title) = data.get("title").and_then(Value::as_str) {
        p.title = title.to_string();
    }
    if let Some(state) = data.get("state").and_then(Value::as_str) {
        p.state = state.to_string();
    }
    if let Some(raw) = data.get("workflow_id").and_then(Value::as_str) {
        if let Some(wf) = normalize_profile_id(raw) {
            p.workflow_id = wf;
        }
    }
    if let Some(raw) = data.get("profile_id").and_then(Value::as_str) {
        if let Some(pid) = normalize_profile_id(raw) {
            p.profile_id = pid;
        }
    }
    p.deferred_from_state = data
        .get("deferred_from_state")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    p.blocked_from_state = data
        .get("blocked_from_state")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    if data.contains_key("invariants") {
        p.invariants = parse_invariants_value(data.get("invariants"));
    }
    if let Some(raw_type) = data.get("type").and_then(Value::as_str) {
        p.knot_type = parse_knot_type(Some(raw_type));
    }
    if data.contains_key("gate") {
        p.gate_data = parse_gate_data_value(data.get("gate"));
    }
    p.created_at = Some(event.occurred_at.clone());
    p.updated_at = event.occurred_at.clone();
}

fn apply_title_set(
    p: &mut RehydrateProjection,
    data: &serde_json::Map<String, Value>,
    event: &FullEvent,
) {
    if let Some(value) = data.get("to").and_then(Value::as_str) {
        p.title = value.to_string();
        p.updated_at = event.occurred_at.clone();
    }
}

fn apply_state_set(
    p: &mut RehydrateProjection,
    data: &serde_json::Map<String, Value>,
    event: &FullEvent,
) {
    if let Some(value) = data.get("to").and_then(Value::as_str) {
        p.state = value.to_string();
        p.updated_at = event.occurred_at.clone();
    }
    p.deferred_from_state = data
        .get("deferred_from_state")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    p.blocked_from_state = data
        .get("blocked_from_state")
        .and_then(Value::as_str)
        .map(ToString::to_string);
}

fn apply_profile_set(
    p: &mut RehydrateProjection,
    data: &serde_json::Map<String, Value>,
    event: &FullEvent,
) {
    let raw_wf = data
        .get("workflow_id")
        .and_then(Value::as_str)
        .or_else(|| data.get("profile_id").and_then(Value::as_str));
    if let Some(raw) = raw_wf {
        if let Some(wf) = normalize_profile_id(raw) {
            p.workflow_id = wf;
        }
    }
    let raw_pid = data
        .get("to_profile_id")
        .and_then(Value::as_str)
        .or_else(|| data.get("profile_id").and_then(Value::as_str));
    if let Some(raw) = raw_pid {
        if let Some(pid) = normalize_profile_id(raw) {
            p.profile_id = pid;
        }
    }
    if let Some(state) = data.get("to_state").and_then(Value::as_str) {
        p.state = state.to_string();
    }
    p.deferred_from_state = data
        .get("deferred_from_state")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    p.blocked_from_state = data
        .get("blocked_from_state")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    p.updated_at = event.occurred_at.clone();
}

fn apply_description_set(
    p: &mut RehydrateProjection,
    data: &serde_json::Map<String, Value>,
    event: &FullEvent,
) {
    let next = data
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    p.description = next.clone();
    p.body = next;
    p.updated_at = event.occurred_at.clone();
}

fn apply_acceptance_set(
    p: &mut RehydrateProjection,
    data: &serde_json::Map<String, Value>,
    event: &FullEvent,
) {
    p.acceptance = data
        .get("acceptance")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    p.updated_at = event.occurred_at.clone();
}

fn apply_priority_set(
    p: &mut RehydrateProjection,
    data: &serde_json::Map<String, Value>,
    event: &FullEvent,
) {
    p.priority = data.get("priority").and_then(Value::as_i64);
    p.updated_at = event.occurred_at.clone();
}

fn apply_type_set(
    p: &mut RehydrateProjection,
    data: &serde_json::Map<String, Value>,
    event: &FullEvent,
) {
    let raw = data.get("type").and_then(Value::as_str);
    p.knot_type = parse_knot_type(raw);
    p.updated_at = event.occurred_at.clone();
}

fn apply_gate_data_set(
    p: &mut RehydrateProjection,
    data: &serde_json::Map<String, Value>,
    event: &FullEvent,
) {
    p.gate_data = parse_gate_data_value(data.get("gate"));
    p.updated_at = event.occurred_at.clone();
}

fn apply_tag_add(p: &mut RehydrateProjection, data: &serde_json::Map<String, Value>) {
    if let Some(tag) = data.get("tag").and_then(Value::as_str) {
        let normalized = tag.trim().to_ascii_lowercase();
        if !normalized.is_empty() && !p.tags.iter().any(|e| e == &normalized) {
            p.tags.push(normalized);
        }
    }
}

fn apply_tag_remove(p: &mut RehydrateProjection, data: &serde_json::Map<String, Value>) {
    if let Some(tag) = data.get("tag").and_then(Value::as_str) {
        let normalized = tag.trim().to_ascii_lowercase();
        p.tags.retain(|existing| existing != &normalized);
    }
}

fn apply_note_added(p: &mut RehydrateProjection, data: &serde_json::Map<String, Value>) {
    if let Some(entry) = parse_metadata_entry_for_rehydrate(data) {
        if !p.notes.iter().any(|e| e.entry_id == entry.entry_id) {
            p.notes.push(entry);
        }
    }
}

fn apply_handoff_capsule_added(p: &mut RehydrateProjection, data: &serde_json::Map<String, Value>) {
    if let Some(entry) = parse_metadata_entry_for_rehydrate(data) {
        if !p
            .handoff_capsules
            .iter()
            .any(|e| e.entry_id == entry.entry_id)
        {
            p.handoff_capsules.push(entry);
        }
    }
}

fn apply_invariants_set(
    p: &mut RehydrateProjection,
    data: &serde_json::Map<String, Value>,
    event: &FullEvent,
) {
    p.invariants = parse_invariants_value(data.get("invariants"));
    p.updated_at = event.occurred_at.clone();
}
