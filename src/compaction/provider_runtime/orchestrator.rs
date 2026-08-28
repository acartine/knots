use super::object_loader::valid_oid;
use super::{CanonicalMode, RuntimeError, SealedProviderEvidence};
use crate::compaction::WriterHead;

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub(crate) struct ControlState {
    pub oid: Option<String>,
    pub epoch: u64,
    pub generation_id: Option<String>,
    pub canonical_oid: Option<String>,
    pub acknowledged_writer_heads: Vec<WriterHead>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum GenerationRefKind {
    Canonical,
    Archive,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct GenerationCandidate {
    pub base_control_oid: Option<String>,
    pub generation_id: String,
    pub canonical_ref: String,
    pub canonical_oid: String,
    pub archive_ref: String,
    pub archive_oid: String,
    pub control_ref: String,
    pub control_oid: String,
}

pub(crate) trait GenerationPlanBuilder {
    fn build(
        &mut self,
        evidence: &SealedProviderEvidence,
        base: &ControlState,
    ) -> Result<GenerationCandidate, RuntimeError>;
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum CasOutcome {
    Applied,
    Stale(ControlState),
}

pub(crate) trait GenerationProvider {
    fn read_control(
        &mut self,
        evidence: &SealedProviderEvidence,
    ) -> Result<ControlState, RuntimeError>;

    fn publish_generation(
        &mut self,
        evidence: &SealedProviderEvidence,
        kind: GenerationRefKind,
        remote_ref: &str,
        expected_oid: Option<&str>,
        oid: &str,
    ) -> Result<(), RuntimeError>;

    fn activate_control(
        &mut self,
        evidence: &SealedProviderEvidence,
        remote_ref: &str,
        expected_oid: Option<&str>,
        control_oid: &str,
    ) -> Result<CasOutcome, RuntimeError>;
}

pub(crate) fn orchestrate_generation(
    provider: &mut impl GenerationProvider,
    builder: &mut impl GenerationPlanBuilder,
    evidence: &SealedProviderEvidence,
    max_attempts: usize,
) -> Result<GenerationCandidate, RuntimeError> {
    if max_attempts == 0 {
        return Err(RuntimeError::ConcurrentControlUpdate);
    }
    let mut base = provider.read_control(evidence)?;
    for _ in 0..max_attempts {
        let candidate = builder.build(evidence, &base)?;
        validate_candidate(evidence, &base, &candidate)?;
        let expected_canonical = match evidence.canonical_mode() {
            CanonicalMode::CreateOnly => None,
            CanonicalMode::IntegratorFastForward => base.canonical_oid.as_deref(),
        };
        provider.publish_generation(
            evidence,
            GenerationRefKind::Canonical,
            &candidate.canonical_ref,
            expected_canonical,
            &candidate.canonical_oid,
        )?;
        provider.publish_generation(
            evidence,
            GenerationRefKind::Archive,
            &candidate.archive_ref,
            None,
            &candidate.archive_oid,
        )?;
        match provider.activate_control(
            evidence,
            &candidate.control_ref,
            base.oid.as_deref(),
            &candidate.control_oid,
        )? {
            CasOutcome::Applied => return Ok(candidate),
            CasOutcome::Stale(observed) => base = observed,
        }
    }
    Err(RuntimeError::ConcurrentControlUpdate)
}

fn validate_candidate(
    evidence: &SealedProviderEvidence,
    base: &ControlState,
    candidate: &GenerationCandidate,
) -> Result<(), RuntimeError> {
    if candidate.base_control_oid != base.oid {
        return Err(RuntimeError::InvalidGeneration(
            "candidate was built from a stale base",
        ));
    }
    if candidate.generation_id.trim().is_empty()
        || !evidence.refs().matches_canonical(&candidate.canonical_ref)
        || !candidate
            .archive_ref
            .starts_with(&evidence.refs().archive_prefix)
        || !valid_oid(&candidate.canonical_oid)
        || !valid_oid(&candidate.archive_oid)
        || !evidence.refs().matches_control(&candidate.control_ref)
        || !valid_oid(&candidate.control_oid)
    {
        return Err(RuntimeError::InvalidGeneration(
            "invalid generation publication plan",
        ));
    }
    Ok(())
}
