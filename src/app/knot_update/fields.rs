use serde_json::json;

use crate::db::KnotCacheRecord;
use crate::events::{new_event_id, FullEvent, FullEventKind};

use crate::app::error::AppError;
use crate::app::helpers::{
    metadata_entry_from_input, non_empty, normalize_tag, require_gate_metadata_scope,
};
use crate::app::types::UpdateKnotPatch;

use super::UpdateState;

pub(crate) fn collect_field_events(
    patch: &UpdateKnotPatch,
    events: &mut Vec<FullEvent>,
    id: &str,
    at: &str,
    us: &mut UpdateState,
    current: &KnotCacheRecord,
) -> Result<(), AppError> {
    collect_title(patch, events, id, at, &mut us.title)?;
    collect_description(patch, events, id, at, &mut us.description, &mut us.body);
    collect_acceptance(patch, events, id, at, &mut us.acceptance);
    collect_priority(patch, events, id, at, &mut us.priority)?;
    collect_type(patch, events, id, at, &mut us.knot_type);
    collect_gate(patch, events, id, at, &mut us.gate_data, us.knot_type)?;
    collect_tags(patch, events, id, at, &mut us.tags);
    collect_invariants(patch, events, id, at, &mut us.invariants, current);
    collect_note(patch, events, id, at, &mut us.notes)?;
    collect_handoff(patch, events, id, at, &mut us.handoff_capsules)?;
    Ok(())
}

fn collect_title(
    patch: &UpdateKnotPatch,
    events: &mut Vec<FullEvent>,
    id: &str,
    at: &str,
    title: &mut String,
) -> Result<(), AppError> {
    if let Some(raw) = patch.title.as_deref() {
        let next = raw.trim();
        if next.is_empty() {
            return Err(AppError::InvalidArgument(
                "title cannot be empty".to_string(),
            ));
        }
        if next != title.as_str() {
            events.push(FullEvent::with_identity(
                new_event_id(),
                at.to_string(),
                id.to_string(),
                FullEventKind::KnotTitleSet.as_str(),
                json!({"from": &*title, "to": next}),
            ));
            *title = next.to_string();
        }
    }
    Ok(())
}

fn collect_description(
    patch: &UpdateKnotPatch,
    events: &mut Vec<FullEvent>,
    id: &str,
    at: &str,
    description: &mut Option<String>,
    body: &mut Option<String>,
) {
    if let Some(raw) = patch.description.as_deref() {
        let next = non_empty(raw);
        if next != *description {
            events.push(FullEvent::with_identity(
                new_event_id(),
                at.to_string(),
                id.to_string(),
                FullEventKind::KnotDescriptionSet.as_str(),
                json!({"description": next}),
            ));
            *description = next;
            *body = description.clone();
        }
    }
}

fn collect_acceptance(
    patch: &UpdateKnotPatch,
    events: &mut Vec<FullEvent>,
    id: &str,
    at: &str,
    acceptance: &mut Option<String>,
) {
    if let Some(raw) = patch.acceptance.as_deref() {
        let next = non_empty(raw).map(|v| v.to_string());
        if next != *acceptance {
            events.push(FullEvent::with_identity(
                new_event_id(),
                at.to_string(),
                id.to_string(),
                FullEventKind::KnotAcceptanceSet.as_str(),
                json!({"acceptance": next}),
            ));
            *acceptance = next;
        }
    }
}

fn collect_priority(
    patch: &UpdateKnotPatch,
    events: &mut Vec<FullEvent>,
    id: &str,
    at: &str,
    priority: &mut Option<i64>,
) -> Result<(), AppError> {
    if let Some(next) = patch.priority {
        if !(0..=4).contains(&next) {
            return Err(AppError::InvalidArgument(
                "priority must be between 0 and 4".to_string(),
            ));
        }
        if *priority != Some(next) {
            events.push(FullEvent::with_identity(
                new_event_id(),
                at.to_string(),
                id.to_string(),
                FullEventKind::KnotPrioritySet.as_str(),
                json!({"priority": next}),
            ));
            *priority = Some(next);
        }
    }
    Ok(())
}

fn collect_type(
    patch: &UpdateKnotPatch,
    events: &mut Vec<FullEvent>,
    id: &str,
    at: &str,
    knot_type: &mut crate::domain::knot_type::KnotType,
) {
    if let Some(next) = patch.knot_type {
        if next != *knot_type {
            events.push(FullEvent::with_identity(
                new_event_id(),
                at.to_string(),
                id.to_string(),
                FullEventKind::KnotTypeSet.as_str(),
                json!({"type": next.as_str()}),
            ));
            *knot_type = next;
        }
    }
}

fn collect_gate(
    patch: &UpdateKnotPatch,
    events: &mut Vec<FullEvent>,
    id: &str,
    at: &str,
    gate_data: &mut crate::domain::gate::GateData,
    knot_type: crate::domain::knot_type::KnotType,
) -> Result<(), AppError> {
    if let Some(owner_kind) = patch.gate_owner_kind {
        gate_data.owner_kind = owner_kind;
    }
    if patch.clear_gate_failure_modes {
        gate_data.failure_modes.clear();
    }
    if let Some(fm) = patch.gate_failure_modes.clone() {
        gate_data.failure_modes = fm;
    }
    if patch.gate_owner_kind.is_some()
        || patch.clear_gate_failure_modes
        || patch.gate_failure_modes.is_some()
    {
        require_gate_metadata_scope(knot_type)?;
        events.push(FullEvent::with_identity(
            new_event_id(),
            at.to_string(),
            id.to_string(),
            FullEventKind::KnotGateDataSet.as_str(),
            json!({"gate": &*gate_data}),
        ));
    }
    Ok(())
}

fn collect_tags(
    patch: &UpdateKnotPatch,
    events: &mut Vec<FullEvent>,
    id: &str,
    at: &str,
    tags: &mut Vec<String>,
) {
    for tag in &patch.add_tags {
        let normalized = normalize_tag(tag);
        if normalized.is_empty() {
            continue;
        }
        if !tags.iter().any(|e| e == &normalized) {
            tags.push(normalized.clone());
            events.push(FullEvent::with_identity(
                new_event_id(),
                at.to_string(),
                id.to_string(),
                FullEventKind::KnotTagAdd.as_str(),
                json!({"tag": normalized}),
            ));
        }
    }
    for tag in &patch.remove_tags {
        let normalized = normalize_tag(tag);
        if normalized.is_empty() {
            continue;
        }
        if tags.iter().any(|e| e == &normalized) {
            tags.retain(|e| e != &normalized);
            events.push(FullEvent::with_identity(
                new_event_id(),
                at.to_string(),
                id.to_string(),
                FullEventKind::KnotTagRemove.as_str(),
                json!({"tag": normalized}),
            ));
        }
    }
}

fn collect_invariants(
    patch: &UpdateKnotPatch,
    events: &mut Vec<FullEvent>,
    id: &str,
    at: &str,
    invariants: &mut Vec<crate::domain::invariant::Invariant>,
    current: &KnotCacheRecord,
) {
    if patch.clear_invariants {
        invariants.clear();
    }
    for inv in &patch.add_invariants {
        if !invariants.iter().any(|e| e == inv) {
            invariants.push(inv.clone());
        }
    }
    for inv in &patch.remove_invariants {
        invariants.retain(|e| e != inv);
    }
    if *invariants != current.invariants {
        events.push(FullEvent::with_identity(
            new_event_id(),
            at.to_string(),
            id.to_string(),
            FullEventKind::KnotInvariantsSet.as_str(),
            json!({"invariants": &*invariants}),
        ));
    }
}

fn collect_note(
    patch: &UpdateKnotPatch,
    events: &mut Vec<FullEvent>,
    id: &str,
    at: &str,
    notes: &mut Vec<crate::domain::metadata::MetadataEntry>,
) -> Result<(), AppError> {
    if let Some(ref input) = patch.add_note {
        let entry = metadata_entry_from_input(input.clone(), at)?;
        if !notes.iter().any(|e| e.entry_id == entry.entry_id) {
            notes.push(entry.clone());
            events.push(FullEvent::with_identity(
                new_event_id(),
                at.to_string(),
                id.to_string(),
                FullEventKind::KnotNoteAdded.as_str(),
                json!({
                    "entry_id": entry.entry_id,
                    "content": entry.content,
                    "username": entry.username,
                    "datetime": entry.datetime,
                    "agentname": entry.agentname,
                    "model": entry.model,
                    "version": entry.version,
                }),
            ));
        }
    }
    Ok(())
}

fn collect_handoff(
    patch: &UpdateKnotPatch,
    events: &mut Vec<FullEvent>,
    id: &str,
    at: &str,
    handoff_capsules: &mut Vec<crate::domain::metadata::MetadataEntry>,
) -> Result<(), AppError> {
    if let Some(ref input) = patch.add_handoff_capsule {
        let entry = metadata_entry_from_input(input.clone(), at)?;
        if !handoff_capsules
            .iter()
            .any(|e| e.entry_id == entry.entry_id)
        {
            handoff_capsules.push(entry.clone());
            events.push(FullEvent::with_identity(
                new_event_id(),
                at.to_string(),
                id.to_string(),
                FullEventKind::KnotHandoffCapsuleAdded.as_str(),
                json!({
                    "entry_id": entry.entry_id,
                    "content": entry.content,
                    "username": entry.username,
                    "datetime": entry.datetime,
                    "agentname": entry.agentname,
                    "model": entry.model,
                    "version": entry.version,
                }),
            ));
        }
    }
    Ok(())
}
