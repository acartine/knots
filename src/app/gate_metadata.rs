use serde_json::json;

use crate::db::{self, KnotCacheRecord, UpsertKnotHot};
use crate::domain::knot_type::parse_knot_type;
use crate::domain::metadata::MetadataEntryInput;
use crate::events::{
    new_event_id, now_utc_rfc3339, EventRecord, FullEvent, FullEventKind, IndexEvent,
    IndexEventKind,
};
use crate::workflow_runtime;

use super::error::AppError;
use super::helpers::{build_knot_head_data, metadata_entry_from_input, KnotHeadData};
use super::types::StateActorMetadata;
use super::App;

impl App {
    pub(crate) fn append_gate_failure_metadata_locked(
        &self,
        current: &KnotCacheRecord,
        gate_id: &str,
        invariant: &str,
        state_actor: &StateActorMetadata,
    ) -> Result<KnotCacheRecord, AppError> {
        let occurred_at = now_utc_rfc3339();
        let message = format!(
            "Gate {} failed invariant '{}' and reopened \
             this knot for planning.",
            gate_id, invariant
        );
        let (note, handoff) = build_gate_failure_entries(&message, state_actor, &occurred_at)?;
        self.write_gate_failure_events(current, &note, &handoff, &occurred_at)?;
        self.persist_gate_failure_cache(current, note, handoff, &occurred_at)
    }

    fn write_gate_failure_events(
        &self,
        current: &KnotCacheRecord,
        note: &crate::domain::metadata::MetadataEntry,
        handoff: &crate::domain::metadata::MetadataEntry,
        occurred_at: &str,
    ) -> Result<(), AppError> {
        let mut note_event = FullEvent::with_identity(
            new_event_id(),
            occurred_at.to_string(),
            current.id.clone(),
            FullEventKind::KnotNoteAdded.as_str(),
            json!({
                "entry_id": note.entry_id,
                "content": note.content,
                "username": note.username,
                "datetime": note.datetime,
                "agentname": note.agentname,
                "model": note.model,
                "version": note.version,
            }),
        );
        let mut handoff_event = FullEvent::with_identity(
            new_event_id(),
            occurred_at.to_string(),
            current.id.clone(),
            FullEventKind::KnotHandoffCapsuleAdded.as_str(),
            json!({
                "entry_id": handoff.entry_id,
                "content": handoff.content,
                "username": handoff.username,
                "datetime": handoff.datetime,
                "agentname": handoff.agentname,
                "model": handoff.model,
                "version": handoff.version,
            }),
        );
        if let Some(expected) = current.profile_etag.as_deref() {
            note_event = note_event.with_precondition(expected);
            handoff_event = handoff_event.with_precondition(expected);
        }
        self.writer.write(&EventRecord::full(note_event))?;
        self.writer.write(&EventRecord::full(handoff_event))?;
        Ok(())
    }

    fn persist_gate_failure_cache(
        &self,
        current: &KnotCacheRecord,
        note: crate::domain::metadata::MetadataEntry,
        handoff: crate::domain::metadata::MetadataEntry,
        occurred_at: &str,
    ) -> Result<KnotCacheRecord, AppError> {
        let mut notes = current.notes.clone();
        notes.push(note);
        let mut handoff_capsules = current.handoff_capsules.clone();
        handoff_capsules.push(handoff);
        let profile = self.resolve_profile_for_record(current)?;
        let knot_type = parse_knot_type(current.knot_type.as_deref());
        let terminal = workflow_runtime::is_terminal_state(
            &self.profile_registry,
            profile.id.as_str(),
            knot_type,
            &current.state,
        )?;
        let index_event_id = new_event_id();
        let mut idx_event = IndexEvent::with_identity(
            index_event_id.clone(),
            occurred_at.to_string(),
            IndexEventKind::KnotHead.as_str(),
            build_knot_head_data(KnotHeadData {
                knot_id: &current.id,
                title: &current.title,
                state: &current.state,
                workflow_id: &profile.workflow_id,
                profile_id: profile.id.as_str(),
                updated_at: occurred_at,
                terminal,
                deferred_from_state: current.deferred_from_state.as_deref(),
                blocked_from_state: current.blocked_from_state.as_deref(),
                invariants: &current.invariants,
                knot_type,
                gate_data: &current.gate_data,
            }),
        );
        if let Some(expected) = current.profile_etag.as_deref() {
            idx_event = idx_event.with_precondition(expected);
        }
        self.writer.write(&EventRecord::index(idx_event))?;
        db::upsert_knot_hot(
            &self.conn,
            &UpsertKnotHot {
                id: &current.id,
                title: &current.title,
                state: &current.state,
                updated_at: occurred_at,
                body: current.body.as_deref(),
                description: current.description.as_deref(),
                acceptance: current.acceptance.as_deref(),
                priority: current.priority,
                knot_type: current.knot_type.as_deref(),
                tags: &current.tags,
                notes: &notes,
                handoff_capsules: &handoff_capsules,
                invariants: &current.invariants,
                step_history: &current.step_history,
                gate_data: &current.gate_data,
                lease_data: &current.lease_data,
                lease_id: current.lease_id.as_deref(),
                workflow_id: &profile.workflow_id,
                profile_id: profile.id.as_str(),
                profile_etag: Some(&index_event_id),
                deferred_from_state: current.deferred_from_state.as_deref(),
                blocked_from_state: current.blocked_from_state.as_deref(),
                created_at: current.created_at.as_deref(),
            },
        )?;
        db::get_knot_hot(&self.conn, &current.id)?
            .ok_or_else(|| AppError::NotFound(current.id.clone()))
    }
}

fn build_gate_failure_entries(
    message: &str,
    state_actor: &StateActorMetadata,
    occurred_at: &str,
) -> Result<
    (
        crate::domain::metadata::MetadataEntry,
        crate::domain::metadata::MetadataEntry,
    ),
    AppError,
> {
    let note = metadata_entry_from_input(
        MetadataEntryInput {
            content: message.to_string(),
            agentname: state_actor.agent_name.clone(),
            model: state_actor.agent_model.clone(),
            version: state_actor.agent_version.clone(),
            ..Default::default()
        },
        occurred_at,
    )?;
    let handoff = metadata_entry_from_input(
        MetadataEntryInput {
            content: message.to_string(),
            agentname: state_actor.agent_name.clone(),
            model: state_actor.agent_model.clone(),
            version: state_actor.agent_version.clone(),
            ..Default::default()
        },
        occurred_at,
    )?;
    Ok((note, handoff))
}
