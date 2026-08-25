use std::collections::{BTreeMap, HashSet};
use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::{
    build_pack, canonical_projection_bytes, projection_descriptor, validate_inbox,
    CanonicalProjections, CompactionManifest, Compatibility, ControlKind, ControlRecord,
    InboxCandidate, InboxError, ProjectionError, RawEvent, SnapshotSet, SourceCheckpoint,
    WriterHead, PROTOCOL_VERSION,
};

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct GenerationBase {
    pub generation_id: Option<String>,
    pub control_epoch: u64,
    pub control_head: Option<String>,
    pub acknowledged_writer_heads: Vec<WriterHead>,
}

#[derive(Debug, Clone)]
pub(crate) struct GenerationInput {
    pub base: GenerationBase,
    pub source: SourceCheckpoint,
    pub snapshots: SnapshotSet,
    pub active_snapshot: Vec<u8>,
    pub cold_snapshot: Vec<u8>,
    pub inboxes: Vec<InboxCandidate>,
    pub action: ControlKind,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConflictRecord {
    pub writer_id: String,
    pub commit: String,
    pub sequence: u64,
    pub submitted_base: Option<String>,
    pub active_generation: Option<String>,
    pub event_ids: Vec<String>,
    pub reason: String,
}

pub(crate) trait ProjectionReplay {
    fn project(
        &self,
        predecessor: &GenerationBase,
        accepted: &[RawEvent],
        conflicts: &[ConflictRecord],
    ) -> Result<CanonicalProjections, String>;

    fn full_replay(
        &self,
        predecessor: &GenerationBase,
        accepted: &[RawEvent],
        conflicts: &[ConflictRecord],
    ) -> Result<CanonicalProjections, String>;
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct GenerationArtifacts {
    pub manifest: CompactionManifest,
    pub active_snapshot: Vec<u8>,
    pub cold_snapshot: Vec<u8>,
    pub projections: Vec<u8>,
    pub pack: super::BuiltPack,
    pub conflicts: Vec<ConflictRecord>,
    pub control: ControlRecord,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum IntegrationError {
    Inbox(InboxError),
    DuplicateBaseWriter,
    WriterSequence,
    WriterChain,
    DuplicateEvent,
    Projection(String),
    ProjectionEncoding(ProjectionError),
    ReplayMismatch,
    Pack(super::PackError),
    InvalidRecovery,
    ControlEpoch,
}

impl fmt::Display for IntegrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "protocol-v2 integration error: {self:?}")
    }
}

impl Error for IntegrationError {}

pub(crate) fn integrate_generation(
    input: GenerationInput,
    replay: &impl ProjectionReplay,
    generation_commit: &str,
    protection_policy_sha256: &str,
) -> Result<GenerationArtifacts, IntegrationError> {
    validate_action(&input)?;
    let mut inboxes = input
        .inboxes
        .iter()
        .map(validate_inbox)
        .collect::<Result<Vec<_>, _>>()
        .map_err(IntegrationError::Inbox)?;
    inboxes.sort_by(|left, right| {
        (&left.writer_id, left.sequence).cmp(&(&right.writer_id, right.sequence))
    });
    let mut vector = base_vector(&input.base.acknowledged_writer_heads)?;
    let mut all_events = Vec::new();
    let mut accepted = Vec::new();
    let mut conflicts = Vec::new();
    let mut event_ids = HashSet::new();
    for inbox in inboxes {
        validate_chain(&inbox, vector.get(&inbox.writer_id))?;
        for event in &inbox.events {
            if !event_ids.insert(event.event_id.clone()) {
                return Err(IntegrationError::DuplicateEvent);
            }
        }
        all_events.extend(inbox.events.clone());
        if inbox.base_generation == input.base.generation_id {
            accepted.extend(inbox.events.clone());
        } else {
            conflicts.push(ConflictRecord {
                writer_id: inbox.writer_id.clone(),
                commit: inbox.commit.clone(),
                sequence: inbox.sequence,
                submitted_base: inbox.base_generation.clone(),
                active_generation: input.base.generation_id.clone(),
                event_ids: inbox
                    .events
                    .iter()
                    .map(|event| event.event_id.clone())
                    .collect(),
                reason: "stale_causal_base".to_string(),
            });
        }
        vector.insert(
            inbox.writer_id.clone(),
            WriterHead {
                writer_id: inbox.writer_id,
                inbox_ref: inbox.inbox_ref,
                commit: inbox.commit,
                sequence: inbox.sequence,
            },
        );
    }
    let projections = projections(replay, &input.base, &accepted, &conflicts)?;
    let projection_bytes =
        canonical_projection_bytes(&projections).map_err(IntegrationError::ProjectionEncoding)?;
    let pack = build_pack(&all_events).map_err(IntegrationError::Pack)?;
    let writer_heads = vector.into_values().collect::<Vec<_>>();
    let manifest = CompactionManifest {
        protocol_version: PROTOCOL_VERSION,
        generation_id: String::new(),
        predecessor_generation: input.base.generation_id.clone(),
        predecessor_control_epoch: input.base.control_epoch,
        source: input.source,
        writer_heads: writer_heads.clone(),
        snapshots: input.snapshots,
        projections: projection_descriptor(&projection_bytes, projections.record_count()),
        packs: vec![pack.descriptor.clone()],
        compatibility: Compatibility {
            minimum_reader_protocol: PROTOCOL_VERSION,
            minimum_writer_protocol: PROTOCOL_VERSION,
        },
    }
    .seal();
    let next_epoch = input
        .base
        .control_epoch
        .checked_add(1)
        .ok_or(IntegrationError::ControlEpoch)?;
    let control = ControlRecord {
        schema_version: 1,
        epoch: next_epoch,
        previous_control_head: input.base.control_head,
        active_generation_id: manifest.generation_id.clone(),
        active_generation_commit: generation_commit.to_string(),
        archive_ref: manifest.archive_ref(),
        acknowledged_writer_heads: writer_heads,
        protection_policy_sha256: protection_policy_sha256.to_string(),
        action: input.action,
    };
    Ok(GenerationArtifacts {
        manifest,
        active_snapshot: input.active_snapshot,
        cold_snapshot: input.cold_snapshot,
        projections: projection_bytes,
        pack,
        conflicts,
        control,
    })
}

fn validate_action(input: &GenerationInput) -> Result<(), IntegrationError> {
    match input.action {
        ControlKind::Activation => Ok(()),
        ControlKind::Recovery { rollback_of_epoch }
            if input.base.control_head.is_some()
                && rollback_of_epoch == input.base.control_epoch =>
        {
            Ok(())
        }
        ControlKind::Recovery { .. } => Err(IntegrationError::InvalidRecovery),
    }
}

fn base_vector(heads: &[WriterHead]) -> Result<BTreeMap<String, WriterHead>, IntegrationError> {
    let mut vector = BTreeMap::new();
    for head in heads {
        if vector
            .insert(head.writer_id.clone(), head.clone())
            .is_some()
        {
            return Err(IntegrationError::DuplicateBaseWriter);
        }
    }
    Ok(vector)
}

fn validate_chain(
    inbox: &super::ValidatedInbox,
    prior: Option<&WriterHead>,
) -> Result<(), IntegrationError> {
    let expected_sequence = match prior {
        Some(head) => head
            .sequence
            .checked_add(1)
            .ok_or(IntegrationError::WriterSequence)?,
        None => 1,
    };
    if inbox.sequence != expected_sequence {
        return Err(IntegrationError::WriterSequence);
    }
    if inbox.previous_head.as_deref() != prior.map(|head| head.commit.as_str()) {
        return Err(IntegrationError::WriterChain);
    }
    Ok(())
}

fn projections(
    replay: &impl ProjectionReplay,
    predecessor: &GenerationBase,
    accepted: &[RawEvent],
    conflicts: &[ConflictRecord],
) -> Result<CanonicalProjections, IntegrationError> {
    let mut projected = replay
        .project(predecessor, accepted, conflicts)
        .map_err(IntegrationError::Projection)?;
    let mut replayed = replay
        .full_replay(predecessor, accepted, conflicts)
        .map_err(IntegrationError::Projection)?;
    let conflict_values = conflicts
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| IntegrationError::Projection(error.to_string()))?;
    projected.conflicts = conflict_values.clone();
    replayed.conflicts = conflict_values;
    let projected_bytes =
        canonical_projection_bytes(&projected).map_err(IntegrationError::ProjectionEncoding)?;
    let replayed_bytes =
        canonical_projection_bytes(&replayed).map_err(IntegrationError::ProjectionEncoding)?;
    if projected_bytes != replayed_bytes {
        return Err(IntegrationError::ReplayMismatch);
    }
    Ok(projected)
}
