use std::collections::BTreeMap;

use crate::compaction::{
    CheckpointInput, CompactionManifest, ControlRecord, SourceCheckpoint, SourceFacts,
    ValidatedProtection,
};

use super::generation_builder::{CONTROL_OBJECT_PATH, MANIFEST_OBJECT_PATH};
use super::{ExactObjectReader, ObjectKind, RuntimeError, SealedProviderEvidence, TreeObject};

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct LoadedCheckpointObjects {
    pub control_ref: String,
    pub control_oid: String,
    pub control: Vec<u8>,
    pub canonical_ref: String,
    pub canonical_oid: String,
    pub archive_ref: String,
    pub manifest: Vec<u8>,
    pub active_snapshot: Vec<u8>,
    pub cold_snapshot: Vec<u8>,
    pub projections: Vec<u8>,
    pub packs: Vec<(String, Vec<u8>)>,
    pub source: SourceCheckpoint,
    predecessor_generation: Option<String>,
    predecessor_control_epoch: u64,
    previous_control_head: Option<String>,
}

impl LoadedCheckpointObjects {
    pub(crate) fn checkpoint_input<'a>(
        &'a self,
        source: SourceFacts<'a>,
        protection: &'a ValidatedProtection,
    ) -> CheckpointInput<'a> {
        CheckpointInput {
            control_head: &self.control_oid,
            control: &self.control,
            manifest: &self.manifest,
            active_snapshot: &self.active_snapshot,
            cold_snapshot: &self.cold_snapshot,
            projections: &self.projections,
            packs: self
                .packs
                .iter()
                .map(|(id, bytes)| (id.as_str(), bytes.as_slice()))
                .collect(),
            source,
            expected_predecessor: self.predecessor_generation.as_deref(),
            expected_previous_control_head: self.previous_control_head.as_deref(),
            expected_control_epoch: self.predecessor_control_epoch,
            expected_archive_ref: &self.archive_ref,
            protection,
        }
    }
}

/// Load one activated checkpoint through exact refs and exact commit OIDs.
///
/// Provider evidence must already be sealed. The loader never checks out a worktree, and it
/// re-reads every authority ref after loading to reject concurrent activation or ref drift.
pub(crate) fn load_checkpoint_objects(
    reader: &impl ExactObjectReader,
    evidence: &SealedProviderEvidence,
    control_ref: &str,
    expected_control_oid: &str,
) -> Result<LoadedCheckpointObjects, RuntimeError> {
    require_ref(evidence.refs().matches_control(control_ref))?;
    require_ref_oid(reader, control_ref, expected_control_oid)?;
    let control_objects = object_map(reader.read_commit_tree(expected_control_oid)?)?;
    let control = required(&control_objects, CONTROL_OBJECT_PATH)?.to_vec();
    let record: ControlRecord = serde_json::from_slice(&control)
        .map_err(|_| RuntimeError::InvalidObject("control record is invalid"))?;

    let canonical_ref = generation_ref(
        &evidence.refs().canonical,
        evidence.refs().canonical_is_prefix,
        &record.active_generation_id,
    );
    require_ref_oid(reader, &canonical_ref, &record.active_generation_commit)?;
    let generation_objects =
        object_map(reader.read_commit_tree(&record.active_generation_commit)?)?;
    let manifest = required(&generation_objects, MANIFEST_OBJECT_PATH)?.to_vec();
    let parsed: CompactionManifest = serde_json::from_slice(&manifest)
        .map_err(|_| RuntimeError::InvalidObject("generation manifest is invalid"))?;
    require_generation_binding(evidence, &record, &parsed)?;

    let archive_ref = format!("{}{}", evidence.refs().archive_prefix, parsed.generation_id);
    require_ref_oid(reader, &archive_ref, &record.active_generation_commit)?;
    let active_snapshot = required(&generation_objects, &parsed.snapshots.active.path)?.to_vec();
    let cold_snapshot = required(&generation_objects, &parsed.snapshots.cold.path)?.to_vec();
    let projections = required(&generation_objects, &parsed.projections.path)?.to_vec();
    let packs = parsed
        .packs
        .iter()
        .map(|pack| {
            required(&generation_objects, &pack.path)
                .map(|bytes| (pack.pack_id.clone(), bytes.to_vec()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    require_ref_oid(reader, control_ref, expected_control_oid)?;
    require_ref_oid(reader, &canonical_ref, &record.active_generation_commit)?;
    require_ref_oid(reader, &archive_ref, &record.active_generation_commit)?;
    Ok(LoadedCheckpointObjects {
        control_ref: control_ref.to_string(),
        control_oid: expected_control_oid.to_string(),
        control,
        canonical_ref,
        canonical_oid: record.active_generation_commit,
        archive_ref,
        manifest,
        active_snapshot,
        cold_snapshot,
        projections,
        packs,
        source: parsed.source,
        predecessor_generation: parsed.predecessor_generation,
        predecessor_control_epoch: parsed.predecessor_control_epoch,
        previous_control_head: record.previous_control_head,
    })
}

fn object_map(objects: Vec<TreeObject>) -> Result<BTreeMap<String, Vec<u8>>, RuntimeError> {
    let mut by_path = BTreeMap::new();
    for object in objects {
        if object.kind != ObjectKind::Blob {
            return Err(RuntimeError::InvalidObject(
                "checkpoint contains a non-blob object",
            ));
        }
        if by_path.insert(object.path, object.bytes).is_some() {
            return Err(RuntimeError::InvalidObject("duplicate checkpoint path"));
        }
    }
    Ok(by_path)
}

fn required<'a>(
    objects: &'a BTreeMap<String, Vec<u8>>,
    path: &str,
) -> Result<&'a [u8], RuntimeError> {
    objects
        .get(path)
        .map(Vec::as_slice)
        .ok_or(RuntimeError::InvalidObject("checkpoint object is missing"))
}

fn require_generation_binding(
    evidence: &SealedProviderEvidence,
    control: &ControlRecord,
    manifest: &CompactionManifest,
) -> Result<(), RuntimeError> {
    let archive_ref = format!(
        "{}{}",
        evidence.refs().archive_prefix,
        manifest.generation_id
    );
    if control.active_generation_id != manifest.generation_id || control.archive_ref != archive_ref
    {
        return Err(RuntimeError::InvalidGeneration(
            "control does not bind the provider generation",
        ));
    }
    Ok(())
}

fn require_ref_oid(
    reader: &impl ExactObjectReader,
    remote_ref: &str,
    expected_oid: &str,
) -> Result<(), RuntimeError> {
    if reader.resolve_ref(remote_ref)?.as_deref() == Some(expected_oid) {
        Ok(())
    } else {
        Err(RuntimeError::RefDrift)
    }
}

fn require_ref(valid: bool) -> Result<(), RuntimeError> {
    valid.then_some(()).ok_or(RuntimeError::InvalidObject(
        "control ref is outside provider authority",
    ))
}

fn generation_ref(prefix_or_ref: &str, is_prefix: bool, generation_id: &str) -> String {
    if is_prefix {
        format!("{prefix_or_ref}{generation_id}")
    } else {
        prefix_or_ref.to_string()
    }
}
