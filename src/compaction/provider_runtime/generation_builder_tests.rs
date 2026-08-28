use std::collections::BTreeMap;
use std::process::Command;

use sha2::{Digest, Sha256};

use super::*;
use crate::compaction::{
    CanonicalProjections, ConflictRecord, ControlRecord, GenerationArtifacts, GenerationBase,
    InboxBundle, ProjectionReplay, RawEvent, SnapshotDescriptor, SnapshotSet, SourceCheckpoint,
};

const GENERATION_OID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CONTROL_OID: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const INBOX_OID: &str = "cccccccccccccccccccccccccccccccccccccccc";
const POLICY: &[u8] = b"enforced provider policy";

#[test]
fn builder_integrates_exact_oid_inbox_and_binds_written_commits() {
    let evidence = diffinite_evidence();
    let active = br#"{"schema_version":1,"hot":[],"warm":[]}"#.to_vec();
    let cold = br#"{"schema_version":1,"cold":[]}"#.to_vec();
    let mut writer = RecordingWriter::default();
    let mut builder = IntegratingGenerationPlanBuilder::new(
        &EmptyReplay,
        &mut writer,
        source(),
        snapshots(&active, &cold),
        active,
        cold,
        vec![loaded_inbox()],
    );
    let candidate = builder
        .build(&evidence, &ControlState::default())
        .expect("generation builds");

    assert_eq!(candidate.canonical_oid, GENERATION_OID);
    assert_eq!(candidate.archive_oid, GENERATION_OID);
    assert_eq!(candidate.control_oid, CONTROL_OID);
    assert_eq!(candidate.canonical_ref, evidence.refs().canonical);
    assert_eq!(candidate.control_ref, evidence.refs().control);
    assert_eq!(
        builder
            .last_artifacts()
            .expect("artifacts retained")
            .control
            .active_generation_commit,
        GENERATION_OID
    );
    assert_eq!(writer.generation_placeholder.as_deref(), Some(ZERO_OID));
    assert_eq!(writer.control_generation.as_deref(), Some(GENERATION_OID));
}

#[test]
fn github_builder_uses_generation_and_epoch_scoped_refs() {
    let evidence = github_evidence();
    let active = b"active".to_vec();
    let cold = b"cold".to_vec();
    let mut writer = RecordingWriter::default();
    let base = ControlState {
        epoch: 6,
        ..ControlState::default()
    };
    let mut builder = IntegratingGenerationPlanBuilder::new(
        &EmptyReplay,
        &mut writer,
        source(),
        snapshots(&active, &cold),
        active,
        cold,
        vec![loaded_inbox()],
    );
    let candidate = builder.build(&evidence, &base).expect("generation builds");

    assert!(candidate
        .canonical_ref
        .starts_with(&evidence.refs().canonical));
    assert_eq!(
        candidate.control_ref,
        format!("{}{:020}", evidence.refs().control, 7)
    );
    assert!(candidate
        .archive_ref
        .starts_with(&evidence.refs().archive_prefix));
}

#[test]
fn builder_writes_generation_and_control_as_real_git_commits() {
    let workspace = knots_test_support::workspace("knots-generation-writer");
    let status = Command::new("git")
        .arg("-C")
        .arg(workspace.path())
        .args(["init", "--bare"])
        .status()
        .expect("git starts");
    assert!(status.success());
    let evidence = diffinite_evidence();
    let mut provider = GitObjectProvider::new(workspace.path());
    let mut builder = IntegratingGenerationPlanBuilder::new(
        &EmptyReplay,
        &mut provider,
        source(),
        snapshots(b"active", b"cold"),
        b"active".to_vec(),
        b"cold".to_vec(),
        vec![loaded_inbox()],
    );

    let candidate = builder
        .build(&evidence, &ControlState::default())
        .expect("generation builds into Git objects");

    assert_eq!(candidate.canonical_oid.len(), 40);
    assert_eq!(candidate.control_oid.len(), 40);
    assert_ne!(candidate.canonical_oid, candidate.control_oid);
}

#[derive(Default)]
struct RecordingWriter {
    generation_placeholder: Option<String>,
    control_generation: Option<String>,
}

impl GenerationObjectWriter for RecordingWriter {
    fn write_generation(
        &mut self,
        artifacts: &GenerationArtifacts,
        _parent_oid: Option<&str>,
    ) -> Result<String, RuntimeError> {
        self.generation_placeholder = Some(artifacts.control.active_generation_commit.clone());
        Ok(GENERATION_OID.to_string())
    }

    fn write_control(&mut self, control: &ControlRecord) -> Result<String, RuntimeError> {
        self.control_generation = Some(control.active_generation_commit.clone());
        Ok(CONTROL_OID.to_string())
    }
}

struct EmptyReplay;

impl ProjectionReplay for EmptyReplay {
    fn project(
        &self,
        _predecessor: &GenerationBase,
        _accepted: &[RawEvent],
        _conflicts: &[ConflictRecord],
    ) -> Result<CanonicalProjections, String> {
        Ok(CanonicalProjections {
            schema_version: 1,
            ..CanonicalProjections::default()
        })
    }

    fn full_replay(
        &self,
        predecessor: &GenerationBase,
        accepted: &[RawEvent],
        conflicts: &[ConflictRecord],
    ) -> Result<CanonicalProjections, String> {
        self.project(predecessor, accepted, conflicts)
    }
}

fn loaded_inbox() -> LoadedInbox {
    let bundle = InboxBundle {
        schema_version: 1,
        writer_id: "writer-a".to_string(),
        sequence: 1,
        previous_head: None,
        base_generation: None,
        entries: Vec::new(),
    };
    LoadedInbox {
        proposal_ref: "refs/heads/knots-v2-inbox/writer-a".to_string(),
        commit_oid: INBOX_OID.to_string(),
        objects: BTreeMap::from([
            (
                ".knots/v2/inbox/submission.json".to_string(),
                b"validated upstream".to_vec(),
            ),
            (
                ".knots/v2/inbox/bundle.json".to_string(),
                serde_json::to_vec(&bundle).expect("bundle serializes"),
            ),
        ]),
    }
}

fn source() -> SourceCheckpoint {
    SourceCheckpoint {
        legacy_ref: "refs/heads/knots".to_string(),
        cutoff_commit: GENERATION_OID.to_string(),
        index_tree: CONTROL_OID.to_string(),
        event_tree: INBOX_OID.to_string(),
    }
}

fn snapshots(active: &[u8], cold: &[u8]) -> SnapshotSet {
    SnapshotSet {
        active: descriptor(".knots/v2/generations/current/active.snapshot.json", active),
        cold: descriptor(".knots/v2/generations/current/cold.snapshot.json", cold),
    }
}

fn descriptor(path: &str, bytes: &[u8]) -> SnapshotDescriptor {
    SnapshotDescriptor {
        path: path.to_string(),
        sha256: digest(bytes),
        bytes: bytes.len() as u64,
        records: 0,
    }
}

fn diffinite_evidence() -> SealedProviderEvidence {
    evidence(ProviderKind::Diffinite, "diffinite:thecartine/quilt")
}

fn github_evidence() -> SealedProviderEvidence {
    evidence(ProviderKind::GitHub, "github:owner/repo")
}

fn evidence(provider: ProviderKind, repository_id: &str) -> SealedProviderEvidence {
    validate_provider_evidence(
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
        },
        POLICY,
        provider,
        repository_id,
    )
    .expect("evidence seals")
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

const ZERO_OID: &str = "0000000000000000000000000000000000000000";
