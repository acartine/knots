use std::collections::BTreeMap;

use crate::compaction::{
    integrate_generation, ControlKind, ControlRecord, GenerationArtifacts, GenerationBase,
    GenerationInput, InboxBundle, InboxCandidate, ProjectionReplay, SnapshotSet, SourceCheckpoint,
};

use super::{
    ControlState, GenerationCandidate, GenerationPlanBuilder, LoadedInbox, RuntimeError,
    SealedProviderEvidence,
};

pub(crate) const MANIFEST_OBJECT_PATH: &str = ".knots/v2/manifest.json";
pub(crate) const CONTROL_OBJECT_PATH: &str = ".knots/v2/control.json";
const UNBOUND_GENERATION_OID: &str = "0000000000000000000000000000000000000000";

/// Writes deterministic generation and control commits into a provider-visible object database.
/// The generation writer must not serialize `artifacts.control`; control is a separate commit.
pub(crate) trait GenerationObjectWriter {
    fn write_generation(
        &mut self,
        artifacts: &GenerationArtifacts,
        parent_oid: Option<&str>,
    ) -> Result<String, RuntimeError>;

    fn write_control(&mut self, control: &ControlRecord) -> Result<String, RuntimeError>;
}

pub(crate) struct IntegratingGenerationPlanBuilder<'a, R, W> {
    replay: &'a R,
    writer: &'a mut W,
    source: SourceCheckpoint,
    snapshots: SnapshotSet,
    active_snapshot: Vec<u8>,
    cold_snapshot: Vec<u8>,
    inboxes: Vec<LoadedInbox>,
    last_artifacts: Option<GenerationArtifacts>,
}

impl<'a, R, W> IntegratingGenerationPlanBuilder<'a, R, W> {
    pub(crate) fn new(
        replay: &'a R,
        writer: &'a mut W,
        source: SourceCheckpoint,
        snapshots: SnapshotSet,
        active_snapshot: Vec<u8>,
        cold_snapshot: Vec<u8>,
        inboxes: Vec<LoadedInbox>,
    ) -> Self {
        Self {
            replay,
            writer,
            source,
            snapshots,
            active_snapshot,
            cold_snapshot,
            inboxes,
            last_artifacts: None,
        }
    }

    pub(crate) fn last_artifacts(&self) -> Option<&GenerationArtifacts> {
        self.last_artifacts.as_ref()
    }
}

impl<R: ProjectionReplay, W: GenerationObjectWriter> GenerationPlanBuilder
    for IntegratingGenerationPlanBuilder<'_, R, W>
{
    fn build(
        &mut self,
        evidence: &SealedProviderEvidence,
        base: &ControlState,
    ) -> Result<GenerationCandidate, RuntimeError> {
        let inboxes = self
            .inboxes
            .iter()
            .map(loaded_candidate)
            .collect::<Result<Vec<_>, _>>()?;
        let input = GenerationInput {
            base: GenerationBase {
                generation_id: base.generation_id.clone(),
                control_epoch: base.epoch,
                control_head: base.oid.clone(),
                acknowledged_writer_heads: base.acknowledged_writer_heads.clone(),
            },
            source: self.source.clone(),
            snapshots: self.snapshots.clone(),
            active_snapshot: self.active_snapshot.clone(),
            cold_snapshot: self.cold_snapshot.clone(),
            inboxes,
            action: ControlKind::Activation,
        };
        let mut artifacts = integrate_generation(
            input,
            self.replay,
            UNBOUND_GENERATION_OID,
            evidence.policy_sha256(),
        )
        .map_err(|_| RuntimeError::InvalidGeneration("generation integration failed"))?;
        let generation_id = artifacts.manifest.generation_id.clone();
        let archive_ref = format!("{}{generation_id}", evidence.refs().archive_prefix);
        let generation_oid = self
            .writer
            .write_generation(&artifacts, base.canonical_oid.as_deref())?;
        require_oid(&generation_oid)?;
        artifacts.control.active_generation_commit = generation_oid.clone();
        artifacts.control.archive_ref = archive_ref.clone();
        let control_oid = self.writer.write_control(&artifacts.control)?;
        require_oid(&control_oid)?;
        let canonical_ref = generation_ref(
            &evidence.refs().canonical,
            evidence.refs().canonical_is_prefix,
            &generation_id,
        );
        let control_ref = if evidence.refs().control_is_prefix {
            format!(
                "{}{epoch:020}",
                evidence.refs().control,
                epoch = artifacts.control.epoch
            )
        } else {
            evidence.refs().control.clone()
        };
        self.last_artifacts = Some(artifacts);
        Ok(GenerationCandidate {
            base_control_oid: base.oid.clone(),
            generation_id,
            canonical_ref,
            canonical_oid: generation_oid.clone(),
            archive_ref,
            archive_oid: generation_oid,
            control_ref,
            control_oid,
        })
    }
}

fn loaded_candidate(loaded: &LoadedInbox) -> Result<InboxCandidate, RuntimeError> {
    let bundle_bytes = loaded
        .objects
        .get(".knots/v2/inbox/bundle.json")
        .ok_or(RuntimeError::InvalidObject("missing inbox bundle"))?;
    let _: InboxBundle = serde_json::from_slice(bundle_bytes)
        .map_err(|_| RuntimeError::InvalidObject("invalid inbox bundle"))?;
    let objects = loaded
        .objects
        .iter()
        .filter(|(path, _)| {
            path.starts_with(".knots/v2/inbox/events/")
                || path.starts_with(".knots/v2/inbox/index/")
        })
        .map(|(path, bytes)| (path.clone(), bytes.clone()))
        .collect::<BTreeMap<_, _>>();
    Ok(InboxCandidate {
        inbox_ref: loaded.proposal_ref.clone(),
        expected_head: loaded.commit_oid.clone(),
        observed_head: loaded.commit_oid.clone(),
        bundle: bundle_bytes.clone(),
        objects,
    })
}

fn generation_ref(prefix_or_ref: &str, is_prefix: bool, generation_id: &str) -> String {
    if is_prefix {
        format!("{prefix_or_ref}{generation_id}")
    } else {
        prefix_or_ref.to_string()
    }
}

fn require_oid(oid: &str) -> Result<(), RuntimeError> {
    super::object_loader::valid_oid(oid)
        .then_some(())
        .ok_or(RuntimeError::InvalidGeneration(
            "object writer returned an invalid oid",
        ))
}
