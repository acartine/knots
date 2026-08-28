use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};

use sha2::{Digest, Sha256};

use super::*;

const OID_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OID_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const OID_C: &str = "cccccccccccccccccccccccccccccccccccccccc";
const POLICY: &[u8] = b"enforced provider policy";

#[test]
fn seals_github_evidence_in_the_heads_namespace() {
    let sealed = evidence(ProviderKind::GitHub, "github:owner/repo").expect("valid GitHub facts");
    assert_eq!(sealed.provider(), ProviderKind::GitHub);
    assert_eq!(sealed.repository_id(), "github:owner/repo");
    assert_eq!(sealed.policy_id(), "policy-1");
    assert_eq!(sealed.integrator_id(), "integrator-1");
    assert_eq!(sealed.refs(), &ProviderRefLayout::github());
    assert_eq!(sealed.control_mode(), ControlMode::ImmutableEpoch);
    assert_eq!(sealed.canonical_mode(), CanonicalMode::CreateOnly);
    assert_eq!(sealed.submission_mode(), SubmissionMode::ImmutableProposal);
    assert_eq!(sealed.policy_sha256(), digest(POLICY));
}

#[test]
fn seals_diffinite_evidence_in_the_work_namespace() {
    let sealed = evidence(ProviderKind::Diffinite, "diffinite:thecartine/quilt")
        .expect("valid Diffinite facts");
    assert_eq!(sealed.provider(), ProviderKind::Diffinite);
    assert_eq!(sealed.refs(), &ProviderRefLayout::diffinite());
    assert_eq!(sealed.control_mode(), ControlMode::IntegratorCompareAndSwap);
    assert_eq!(
        sealed.canonical_mode(),
        CanonicalMode::IntegratorFastForward
    );
    assert_eq!(sealed.submission_mode(), SubmissionMode::CredentialInbox);
}

#[test]
fn evidence_rejects_repository_policy_and_namespace_substitution() {
    let mut input = evidence_input(ProviderKind::Diffinite, "diffinite:thecartine/quilt");
    let error = validate_provider_evidence(
        input.clone(),
        POLICY,
        ProviderKind::Diffinite,
        "diffinite:somewhere/else",
    )
    .expect_err("repository substitution must fail");
    assert!(matches!(error, RuntimeError::InvalidEvidence(_)));

    input.policy_sha256 = digest(b"different policy");
    assert!(validate_provider_evidence(
        input.clone(),
        POLICY,
        ProviderKind::Diffinite,
        "diffinite:thecartine/quilt"
    )
    .is_err());

    input.policy_sha256 = digest(POLICY);
    input.refs = ProviderRefLayout::github();
    assert!(validate_provider_evidence(
        input,
        POLICY,
        ProviderKind::Diffinite,
        "diffinite:thecartine/quilt"
    )
    .is_err());
}

#[test]
fn exact_oid_loader_accepts_only_blob_objects_without_checkout() {
    let refs = ProviderRefLayout::github();
    let proposal_ref = format!("{}writer/1/{OID_A}", refs.submission_prefix);
    let reader = FakeReader::stable(
        OID_A,
        vec![
            blob(".knots/v2/inbox/submission.json", b"submission"),
            blob(".knots/v2/inbox/bundle.json", b"bundle"),
            blob(".knots/v2/inbox/events/event.json", b"event"),
        ],
    );
    let loaded = load_untrusted_inbox(&reader, &refs, &proposal_ref, OID_A)
        .expect("exact commit tree should load");
    assert_eq!(loaded.commit_oid, OID_A);
    assert_eq!(loaded.objects.len(), 3);
    assert_eq!(*reader.reads.borrow(), 1);
}

#[test]
fn exact_oid_loader_rejects_ref_drift() {
    let refs = ProviderRefLayout::github();
    let proposal_ref = format!("{}writer/1/{OID_A}", refs.submission_prefix);
    let reader = FakeReader {
        refs: RefCell::new(VecDeque::from([
            Some(OID_A.to_string()),
            Some(OID_B.to_string()),
        ])),
        objects: valid_objects(),
        reads: RefCell::new(0),
    };
    assert_eq!(
        load_untrusted_inbox(&reader, &refs, &proposal_ref, OID_A),
        Err(RuntimeError::RefDrift)
    );
}

#[test]
fn exact_oid_loader_rejects_object_type_and_path_attacks() {
    let refs = ProviderRefLayout::github();
    let proposal_ref = format!("{}writer/1/{OID_A}", refs.submission_prefix);
    let mut wrong_kind = valid_objects();
    wrong_kind.push(TreeObject {
        path: ".knots/v2/inbox/events/tree".to_string(),
        kind: ObjectKind::Symlink,
        oid: OID_B.to_string(),
        bytes: b"target".to_vec(),
    });
    assert!(matches!(
        load_untrusted_inbox(
            &FakeReader::stable(OID_A, wrong_kind),
            &refs,
            &proposal_ref,
            OID_A
        ),
        Err(RuntimeError::InvalidObject(_))
    ));

    let mut traversal = valid_objects();
    traversal.push(blob(".knots/v2/inbox/events/../../escape.json", b"bad"));
    assert!(matches!(
        load_untrusted_inbox(
            &FakeReader::stable(OID_A, traversal),
            &refs,
            &proposal_ref,
            OID_A
        ),
        Err(RuntimeError::InvalidObject(_))
    ));
}

#[test]
fn concurrent_control_cas_rebuilds_from_the_observed_head() {
    let sealed = evidence(ProviderKind::Diffinite, "diffinite:thecartine/quilt").unwrap();
    let stale = ControlState {
        oid: Some(OID_B.to_string()),
        epoch: 8,
        generation_id: Some("other-generation".to_string()),
        canonical_oid: Some(OID_A.to_string()),
        acknowledged_writer_heads: Vec::new(),
    };
    let mut provider = FakeProvider::new(ControlState::default());
    provider.stale_once = Some(stale.clone());
    let mut builder = FakeBuilder::default();
    let result = orchestrate_generation(&mut provider, &mut builder, &sealed, 2)
        .expect("stale CAS should rebuild once");
    assert_eq!(result.base_control_oid, stale.oid);
    assert_eq!(builder.bases, vec![None, Some(OID_B.to_string())]);
    assert_eq!(
        provider.canonical_expectations,
        vec![None, Some(OID_A.to_string())]
    );
    assert_eq!(
        provider.actions,
        vec!["canonical", "archive", "cas", "canonical", "archive", "cas"]
    );
}

#[test]
fn interrupted_publication_recovers_without_activating_partial_state() {
    let sealed = evidence(ProviderKind::GitHub, "github:owner/repo").unwrap();
    let mut provider = FakeProvider::new(ControlState::default());
    provider.fail_archive_once = true;
    let mut builder = FakeBuilder::default();
    assert!(orchestrate_generation(&mut provider, &mut builder, &sealed, 1).is_err());
    assert!(!provider.actions.contains(&"cas"));

    let result = orchestrate_generation(&mut provider, &mut builder, &sealed, 1)
        .expect("recovery republishes immutable objects idempotently");
    assert_eq!(result.generation_id, "generation-1");
    assert_eq!(provider.control.oid.as_deref(), Some(OID_C));
}

fn evidence(
    provider: ProviderKind,
    repository_id: &str,
) -> Result<SealedProviderEvidence, RuntimeError> {
    validate_provider_evidence(
        evidence_input(provider, repository_id),
        POLICY,
        provider,
        repository_id,
    )
}

fn evidence_input(provider: ProviderKind, repository_id: &str) -> ProviderEvidenceInput {
    ProviderEvidenceInput {
        provider,
        repository_id: repository_id.to_string(),
        policy_id: "policy-1".to_string(),
        policy_sha256: digest(POLICY),
        integrator_id: "integrator-1".to_string(),
        refs: match provider {
            ProviderKind::GitHub => ProviderRefLayout::github(),
            ProviderKind::Diffinite => ProviderRefLayout::diffinite(),
        },
        control_mode: match provider {
            ProviderKind::GitHub => ControlMode::ImmutableEpoch,
            ProviderKind::Diffinite => ControlMode::IntegratorCompareAndSwap,
        },
        canonical_mode: match provider {
            ProviderKind::GitHub => CanonicalMode::CreateOnly,
            ProviderKind::Diffinite => CanonicalMode::IntegratorFastForward,
        },
        archives_create_only: true,
        submission_mode: match provider {
            ProviderKind::GitHub => SubmissionMode::ImmutableProposal,
            ProviderKind::Diffinite => SubmissionMode::CredentialInbox,
        },
        exact_oid_reads: true,
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn blob(path: &str, bytes: &[u8]) -> TreeObject {
    TreeObject {
        path: path.to_string(),
        kind: ObjectKind::Blob,
        oid: OID_B.to_string(),
        bytes: bytes.to_vec(),
    }
}

fn valid_objects() -> Vec<TreeObject> {
    vec![
        blob(".knots/v2/inbox/submission.json", b"submission"),
        blob(".knots/v2/inbox/bundle.json", b"bundle"),
    ]
}

struct FakeReader {
    refs: RefCell<VecDeque<Option<String>>>,
    objects: Vec<TreeObject>,
    reads: RefCell<usize>,
}

impl FakeReader {
    fn stable(oid: &str, objects: Vec<TreeObject>) -> Self {
        Self {
            refs: RefCell::new(VecDeque::from([
                Some(oid.to_string()),
                Some(oid.to_string()),
            ])),
            objects,
            reads: RefCell::new(0),
        }
    }
}

impl ExactObjectReader for FakeReader {
    fn resolve_ref(&self, _remote_ref: &str) -> Result<Option<String>, RuntimeError> {
        Ok(self.refs.borrow_mut().pop_front().flatten())
    }

    fn read_commit_tree(&self, _commit_oid: &str) -> Result<Vec<TreeObject>, RuntimeError> {
        *self.reads.borrow_mut() += 1;
        Ok(self.objects.clone())
    }
}

#[derive(Default)]
struct FakeBuilder {
    bases: Vec<Option<String>>,
}

impl GenerationPlanBuilder for FakeBuilder {
    fn build(
        &mut self,
        evidence: &SealedProviderEvidence,
        base: &ControlState,
    ) -> Result<GenerationCandidate, RuntimeError> {
        self.bases.push(base.oid.clone());
        let refs = evidence.refs();
        let canonical_ref = if refs.canonical_is_prefix {
            format!("{}generation-1/nonce", refs.canonical)
        } else {
            refs.canonical.clone()
        };
        let control_ref = if refs.control_is_prefix {
            format!("{}0001/run-nonce-digest", refs.control)
        } else {
            refs.control.clone()
        };
        Ok(GenerationCandidate {
            base_control_oid: base.oid.clone(),
            generation_id: "generation-1".to_string(),
            canonical_ref,
            canonical_oid: OID_A.to_string(),
            archive_ref: format!("{}generation-1/nonce", evidence.refs().archive_prefix),
            archive_oid: OID_A.to_string(),
            control_ref,
            control_oid: OID_C.to_string(),
        })
    }
}

struct FakeProvider {
    control: ControlState,
    immutable: BTreeMap<String, String>,
    actions: Vec<&'static str>,
    canonical_expectations: Vec<Option<String>>,
    stale_once: Option<ControlState>,
    fail_archive_once: bool,
}

impl FakeProvider {
    fn new(control: ControlState) -> Self {
        Self {
            control,
            immutable: BTreeMap::new(),
            actions: Vec::new(),
            canonical_expectations: Vec::new(),
            stale_once: None,
            fail_archive_once: false,
        }
    }
}

impl GenerationProvider for FakeProvider {
    fn read_control(
        &mut self,
        _evidence: &SealedProviderEvidence,
    ) -> Result<ControlState, RuntimeError> {
        Ok(self.control.clone())
    }

    fn publish_generation(
        &mut self,
        _evidence: &SealedProviderEvidence,
        kind: GenerationRefKind,
        remote_ref: &str,
        expected_oid: Option<&str>,
        oid: &str,
    ) -> Result<(), RuntimeError> {
        self.actions.push(match kind {
            GenerationRefKind::Canonical => "canonical",
            GenerationRefKind::Archive => "archive",
        });
        if kind == GenerationRefKind::Canonical {
            self.canonical_expectations
                .push(expected_oid.map(str::to_string));
        }
        if kind == GenerationRefKind::Archive && self.fail_archive_once {
            self.fail_archive_once = false;
            return Err(RuntimeError::Provider(
                "interrupted archive publication".to_string(),
            ));
        }
        match self.immutable.get(remote_ref) {
            Some(existing) if existing != oid => {
                Err(RuntimeError::Provider("immutable ref conflict".to_string()))
            }
            Some(_) => Ok(()),
            None => {
                self.immutable
                    .insert(remote_ref.to_string(), oid.to_string());
                Ok(())
            }
        }
    }

    fn activate_control(
        &mut self,
        _evidence: &SealedProviderEvidence,
        _remote_ref: &str,
        expected_oid: Option<&str>,
        control_oid: &str,
    ) -> Result<CasOutcome, RuntimeError> {
        self.actions.push("cas");
        if let Some(stale) = self.stale_once.take() {
            self.control = stale.clone();
            return Ok(CasOutcome::Stale(stale));
        }
        if self.control.oid.as_deref() != expected_oid {
            return Ok(CasOutcome::Stale(self.control.clone()));
        }
        self.control.oid = Some(control_oid.to_string());
        self.control.epoch += 1;
        Ok(CasOutcome::Applied)
    }
}
