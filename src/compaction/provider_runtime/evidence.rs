use sha2::{Digest, Sha256};

use super::RuntimeError;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ProviderKind {
    GitHub,
    Diffinite,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ControlMode {
    ImmutableEpoch,
    IntegratorCompareAndSwap,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum CanonicalMode {
    CreateOnly,
    IntegratorFastForward,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum SubmissionMode {
    ImmutableProposal,
    CredentialInbox,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ProviderRefLayout {
    pub control: String,
    pub control_is_prefix: bool,
    pub canonical: String,
    pub canonical_is_prefix: bool,
    pub archive_prefix: String,
    pub submission_prefix: String,
}

impl ProviderRefLayout {
    pub(crate) fn github() -> Self {
        Self {
            control: "refs/heads/knots-v2-control/epochs/".to_string(),
            control_is_prefix: true,
            canonical: "refs/heads/knots-v2-canonical/".to_string(),
            canonical_is_prefix: true,
            archive_prefix: "refs/heads/knots-v2-archive/".to_string(),
            submission_prefix: "refs/heads/knots-v2-proposals/".to_string(),
        }
    }

    pub(crate) fn diffinite() -> Self {
        Self {
            control: "refs/work/knots-v2/control".to_string(),
            control_is_prefix: false,
            canonical: "refs/work/knots-v2/canonical".to_string(),
            canonical_is_prefix: false,
            archive_prefix: "refs/work/knots-v2/archive/".to_string(),
            submission_prefix: "refs/work/knots-v2/inbox/".to_string(),
        }
    }

    pub(crate) fn matches_control(&self, remote_ref: &str) -> bool {
        matches_ref(remote_ref, &self.control, self.control_is_prefix)
    }

    pub(crate) fn matches_canonical(&self, remote_ref: &str) -> bool {
        matches_ref(remote_ref, &self.canonical, self.canonical_is_prefix)
    }
}

fn matches_ref(remote_ref: &str, configured: &str, is_prefix: bool) -> bool {
    if is_prefix {
        remote_ref.starts_with(configured) && remote_ref.len() > configured.len()
    } else {
        remote_ref == configured
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderEvidenceInput {
    pub provider: ProviderKind,
    pub repository_id: String,
    pub policy_id: String,
    pub policy_sha256: String,
    pub integrator_id: String,
    pub refs: ProviderRefLayout,
    pub control_mode: ControlMode,
    pub canonical_mode: CanonicalMode,
    pub archives_create_only: bool,
    pub submission_mode: SubmissionMode,
    pub exact_oid_reads: bool,
}

/// Provider facts that passed identity, policy-byte, namespace, and enforcement checks.
///
/// Fields stay private so publication code cannot construct authority from configuration alone.
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct SealedProviderEvidence {
    provider: ProviderKind,
    repository_id: String,
    policy_id: String,
    policy_sha256: String,
    integrator_id: String,
    refs: ProviderRefLayout,
    control_mode: ControlMode,
    canonical_mode: CanonicalMode,
    submission_mode: SubmissionMode,
}

impl SealedProviderEvidence {
    pub(crate) fn provider(&self) -> ProviderKind {
        self.provider
    }
    pub(crate) fn repository_id(&self) -> &str {
        &self.repository_id
    }
    pub(crate) fn policy_id(&self) -> &str {
        &self.policy_id
    }
    pub(crate) fn policy_sha256(&self) -> &str {
        &self.policy_sha256
    }
    pub(crate) fn integrator_id(&self) -> &str {
        &self.integrator_id
    }
    pub(crate) fn refs(&self) -> &ProviderRefLayout {
        &self.refs
    }
    pub(crate) fn control_mode(&self) -> ControlMode {
        self.control_mode
    }
    pub(crate) fn canonical_mode(&self) -> CanonicalMode {
        self.canonical_mode
    }
    pub(crate) fn submission_mode(&self) -> SubmissionMode {
        self.submission_mode
    }
}

pub(crate) fn validate_provider_evidence(
    input: ProviderEvidenceInput,
    policy_bytes: &[u8],
    expected_provider: ProviderKind,
    expected_repository_id: &str,
) -> Result<SealedProviderEvidence, RuntimeError> {
    if input.provider != expected_provider || input.repository_id != expected_repository_id {
        return Err(RuntimeError::InvalidEvidence(
            "provider or repository substitution",
        ));
    }
    if input.repository_id.trim().is_empty()
        || input.policy_id.trim().is_empty()
        || input.integrator_id.trim().is_empty()
    {
        return Err(RuntimeError::InvalidEvidence("empty provider identity"));
    }
    if input.policy_sha256 != format!("{:x}", Sha256::digest(policy_bytes)) {
        return Err(RuntimeError::InvalidEvidence("policy digest mismatch"));
    }
    let (refs, control_mode, canonical_mode, submission_mode) = expected_contract(input.provider);
    if input.refs != refs {
        return Err(RuntimeError::InvalidEvidence(
            "provider ref namespace mismatch",
        ));
    }
    if input.control_mode != control_mode
        || input.canonical_mode != canonical_mode
        || input.submission_mode != submission_mode
        || !input.archives_create_only
        || !input.exact_oid_reads
    {
        return Err(RuntimeError::InvalidEvidence(
            "required provider enforcement is absent",
        ));
    }
    Ok(SealedProviderEvidence {
        provider: input.provider,
        repository_id: input.repository_id,
        policy_id: input.policy_id,
        policy_sha256: input.policy_sha256,
        integrator_id: input.integrator_id,
        refs: input.refs,
        control_mode: input.control_mode,
        canonical_mode: input.canonical_mode,
        submission_mode: input.submission_mode,
    })
}

fn expected_contract(
    provider: ProviderKind,
) -> (
    ProviderRefLayout,
    ControlMode,
    CanonicalMode,
    SubmissionMode,
) {
    match provider {
        ProviderKind::GitHub => (
            ProviderRefLayout::github(),
            ControlMode::ImmutableEpoch,
            CanonicalMode::CreateOnly,
            SubmissionMode::ImmutableProposal,
        ),
        ProviderKind::Diffinite => (
            ProviderRefLayout::diffinite(),
            ControlMode::IntegratorCompareAndSwap,
            CanonicalMode::IntegratorFastForward,
            SubmissionMode::CredentialInbox,
        ),
    }
}
