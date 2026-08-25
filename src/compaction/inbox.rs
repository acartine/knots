use std::collections::{BTreeMap, HashSet};
use std::error::Error;
use std::fmt;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::events::{EventRecord, EventStream};

use super::{RawEvent, V2RefLayout};

pub(crate) const INBOX_DATA_ROOT: &str = ".knots/v2/inbox";

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InboxEntry {
    pub path: String,
    pub event_id: String,
    pub content_sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InboxBundle {
    pub schema_version: u32,
    pub writer_id: String,
    pub sequence: u64,
    pub previous_head: Option<String>,
    pub base_generation: Option<String>,
    pub entries: Vec<InboxEntry>,
}

#[derive(Debug, Clone)]
pub(crate) struct InboxCandidate {
    pub inbox_ref: String,
    pub expected_head: String,
    pub observed_head: String,
    pub bundle: Vec<u8>,
    pub objects: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ValidatedInbox {
    pub writer_id: String,
    pub inbox_ref: String,
    pub commit: String,
    pub sequence: u64,
    pub previous_head: Option<String>,
    pub base_generation: Option<String>,
    pub events: Vec<RawEvent>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum InboxError {
    InvalidBundle,
    InvalidWriter,
    InvalidHead,
    InvalidSequence,
    InvalidPath,
    MissingObject,
    LengthMismatch,
    HashMismatch,
    EventSchema,
    InvalidEventId,
    EventIdMismatch,
    StreamMismatch,
    DuplicatePath,
    DuplicateEvent,
    UndeclaredObject,
}

impl fmt::Display for InboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid protocol-v2 inbox: {self:?}")
    }
}

impl Error for InboxError {}

pub(crate) fn validate_inbox(candidate: &InboxCandidate) -> Result<ValidatedInbox, InboxError> {
    let bundle: InboxBundle =
        serde_json::from_slice(&candidate.bundle).map_err(|_| InboxError::InvalidBundle)?;
    validate_authority(candidate, &bundle)?;
    let mut paths = HashSet::new();
    let mut event_ids = HashSet::new();
    let mut events = Vec::with_capacity(bundle.entries.len());
    for entry in &bundle.entries {
        if entry.event_id.is_empty()
            || entry.event_id.len() > 128
            || !entry
                .event_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(InboxError::InvalidEventId);
        }
        if !paths.insert(entry.path.as_str()) {
            return Err(InboxError::DuplicatePath);
        }
        if !event_ids.insert(entry.event_id.as_str()) {
            return Err(InboxError::DuplicateEvent);
        }
        let stream = path_stream(&entry.path).ok_or(InboxError::InvalidPath)?;
        let bytes = candidate
            .objects
            .get(&entry.path)
            .ok_or(InboxError::MissingObject)?;
        validate_entry(entry, bytes, stream)?;
        events.push(RawEvent {
            path: entry.path.clone(),
            event_id: entry.event_id.clone(),
            bytes: bytes.clone(),
        });
    }
    if candidate.objects.len() != paths.len()
        || candidate
            .objects
            .keys()
            .any(|path| !paths.contains(path.as_str()))
    {
        return Err(InboxError::UndeclaredObject);
    }
    events.sort();
    Ok(ValidatedInbox {
        writer_id: bundle.writer_id,
        inbox_ref: candidate.inbox_ref.clone(),
        commit: candidate.observed_head.clone(),
        sequence: bundle.sequence,
        previous_head: bundle.previous_head,
        base_generation: bundle.base_generation,
        events,
    })
}

fn validate_authority(candidate: &InboxCandidate, bundle: &InboxBundle) -> Result<(), InboxError> {
    if bundle.schema_version != 1 {
        return Err(InboxError::InvalidBundle);
    }
    if !valid_writer(&bundle.writer_id)
        || candidate.inbox_ref != V2RefLayout::default().inbox(&bundle.writer_id)
    {
        return Err(InboxError::InvalidWriter);
    }
    if bundle.sequence == 0 {
        return Err(InboxError::InvalidSequence);
    }
    if candidate.expected_head != candidate.observed_head
        || !valid_object_id(&candidate.observed_head)
        || bundle
            .previous_head
            .as_deref()
            .is_some_and(|head| !valid_object_id(head))
    {
        return Err(InboxError::InvalidHead);
    }
    Ok(())
}

fn validate_entry(
    entry: &InboxEntry,
    bytes: &[u8],
    expected_stream: EventStream,
) -> Result<(), InboxError> {
    if entry.bytes != bytes.len() as u64 {
        return Err(InboxError::LengthMismatch);
    }
    let digest = format!("{:x}", Sha256::digest(bytes));
    if entry.content_sha256 != digest {
        return Err(InboxError::HashMismatch);
    }
    let event: EventRecord = serde_json::from_slice(bytes).map_err(|_| InboxError::EventSchema)?;
    validate_event_schema(bytes, &event)?;
    if event.event_id() != entry.event_id {
        return Err(InboxError::EventIdMismatch);
    }
    if event.stream() != expected_stream {
        return Err(InboxError::StreamMismatch);
    }
    Ok(())
}

fn validate_event_schema(bytes: &[u8], event: &EventRecord) -> Result<(), InboxError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| InboxError::EventSchema)?;
    let object = value.as_object().ok_or(InboxError::EventSchema)?;
    let allowed = [
        "event_id",
        "occurred_at",
        "knot_id",
        "type",
        "data",
        "precondition",
    ];
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(InboxError::EventSchema);
    }
    let valid_type = match event {
        EventRecord::Full(event) => matches!(
            event.event_type.as_str(),
            "knot.created"
                | "knot.title_set"
                | "knot.body_set"
                | "knot.description_set"
                | "knot.acceptance_set"
                | "knot.state_set"
                | "knot.priority_set"
                | "knot.type_set"
                | "knot.comment_added"
                | "knot.note_added"
                | "knot.handoff_capsule_added"
                | "knot.tag_add"
                | "knot.tag_remove"
                | "knot.invariants_set"
                | "knot.verification_steps_set"
                | "knot.gate_data_set"
                | "knot.execution_plan_data_set"
                | "knot.scope_set"
                | "knot.edge_add"
                | "knot.edge_remove"
                | "knot.profile_set"
                | "knot.review_decision"
                | "knot.lease_data_set"
                | "knot.lease_id_set"
        ),
        EventRecord::Index(event) => event.event_type == "idx.knot_head",
    };
    valid_type.then_some(()).ok_or(InboxError::EventSchema)
}

fn path_stream(value: &str) -> Option<EventStream> {
    let path = Path::new(value);
    if !path.starts_with(INBOX_DATA_ROOT)
        || path.extension().and_then(|value| value.to_str()) != Some("json")
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return None;
    }
    let relative = path.strip_prefix(INBOX_DATA_ROOT).ok()?;
    match relative.components().next()? {
        Component::Normal(value) if value == "events" => Some(EventStream::Full),
        Component::Normal(value) if value == "index" => Some(EventStream::Index),
        _ => None,
    }
}

fn valid_writer(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
