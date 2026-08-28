use std::cell::RefCell;
use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use super::*;
use crate::compaction::{
    CompactionManifest, Compatibility, ControlKind, ControlRecord, SnapshotDescriptor, SnapshotSet,
    SourceCheckpoint, LEGACY_REF, PROTOCOL_VERSION,
};

const CONTROL_OID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const GENERATION_OID: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const BLOB_OID: &str = "cccccccccccccccccccccccccccccccccccccccc";
const POLICY: &[u8] = b"diffinite hook policy";

#[test]
fn diffinite_loader_reads_fixed_work_refs_and_manifest_paths() {
    let evidence = evidence();
    let fixture = fixture(&evidence);
    let loaded = load_checkpoint_objects(
        &fixture.reader,
        &evidence,
        &evidence.refs().control,
        CONTROL_OID,
    )
    .expect("valid activated checkpoint loads");

    assert_eq!(loaded.canonical_ref, "refs/work/knots-v2/canonical");
    assert!(loaded
        .archive_ref
        .starts_with("refs/work/knots-v2/archive/"));
    assert_eq!(loaded.active_snapshot, b"active");
    assert_eq!(loaded.cold_snapshot, b"cold");
    assert_eq!(loaded.projections, b"projections");
    assert!(loaded.packs.is_empty());
    assert_eq!(*fixture.reader.tree_reads.borrow(), 2);
}

#[test]
fn loader_rejects_control_bound_to_the_wrong_provider_archive() {
    let evidence = evidence();
    let mut fixture = fixture(&evidence);
    let mut control: ControlRecord =
        serde_json::from_slice(&fixture.trees[CONTROL_OID][0].bytes).expect("control fixture");
    control.archive_ref = format!(
        "refs/heads/knots-v2-archive/{}",
        control.active_generation_id
    );
    fixture.trees.get_mut(CONTROL_OID).unwrap()[0].bytes = serde_json::to_vec(&control).unwrap();
    fixture.reader.trees = fixture.trees;

    assert!(matches!(
        load_checkpoint_objects(
            &fixture.reader,
            &evidence,
            &evidence.refs().control,
            CONTROL_OID
        ),
        Err(RuntimeError::InvalidGeneration(_))
    ));
}

struct Fixture {
    reader: FakeReader,
    trees: BTreeMap<String, Vec<TreeObject>>,
}

fn fixture(evidence: &SealedProviderEvidence) -> Fixture {
    let manifest = manifest();
    let archive_ref = format!(
        "{}{}",
        evidence.refs().archive_prefix,
        manifest.generation_id
    );
    let control = ControlRecord {
        schema_version: 1,
        epoch: 1,
        previous_control_head: None,
        active_generation_id: manifest.generation_id.clone(),
        active_generation_commit: GENERATION_OID.to_string(),
        archive_ref: archive_ref.clone(),
        acknowledged_writer_heads: vec![],
        protection_policy_sha256: digest(POLICY),
        action: ControlKind::Activation,
    };
    let mut refs = BTreeMap::new();
    refs.insert(evidence.refs().control.clone(), CONTROL_OID.to_string());
    refs.insert(
        evidence.refs().canonical.clone(),
        GENERATION_OID.to_string(),
    );
    refs.insert(archive_ref, GENERATION_OID.to_string());
    let mut trees = BTreeMap::new();
    trees.insert(
        CONTROL_OID.to_string(),
        vec![blob(
            CONTROL_OBJECT_PATH,
            serde_json::to_vec(&control).unwrap(),
        )],
    );
    trees.insert(
        GENERATION_OID.to_string(),
        vec![
            blob(MANIFEST_OBJECT_PATH, serde_json::to_vec(&manifest).unwrap()),
            blob(&manifest.snapshots.active.path, b"active".to_vec()),
            blob(&manifest.snapshots.cold.path, b"cold".to_vec()),
            blob(&manifest.projections.path, b"projections".to_vec()),
        ],
    );
    let reader = FakeReader {
        refs,
        trees: trees.clone(),
        tree_reads: RefCell::new(0),
    };
    Fixture { reader, trees }
}

fn manifest() -> CompactionManifest {
    CompactionManifest {
        protocol_version: PROTOCOL_VERSION,
        generation_id: String::new(),
        predecessor_generation: None,
        predecessor_control_epoch: 0,
        source: SourceCheckpoint {
            legacy_ref: LEGACY_REF.to_string(),
            cutoff_commit: BLOB_OID.to_string(),
            index_tree: BLOB_OID.to_string(),
            event_tree: BLOB_OID.to_string(),
        },
        writer_heads: vec![],
        snapshots: SnapshotSet {
            active: descriptor(".knots/v2/generations/current/active.snapshot.json"),
            cold: descriptor(".knots/v2/generations/current/cold.snapshot.json"),
        },
        projections: descriptor(".knots/v2/generations/current/state.projections.json"),
        packs: vec![],
        compatibility: Compatibility {
            minimum_reader_protocol: PROTOCOL_VERSION,
            minimum_writer_protocol: PROTOCOL_VERSION,
        },
    }
    .seal()
}

fn descriptor(path: &str) -> SnapshotDescriptor {
    SnapshotDescriptor {
        path: path.to_string(),
        sha256: digest(b"unused by object loading"),
        bytes: 0,
        records: 0,
    }
}

fn blob(path: &str, bytes: Vec<u8>) -> TreeObject {
    TreeObject {
        path: path.to_string(),
        kind: ObjectKind::Blob,
        oid: BLOB_OID.to_string(),
        bytes,
    }
}

struct FakeReader {
    refs: BTreeMap<String, String>,
    trees: BTreeMap<String, Vec<TreeObject>>,
    tree_reads: RefCell<usize>,
}

impl ExactObjectReader for FakeReader {
    fn resolve_ref(&self, remote_ref: &str) -> Result<Option<String>, RuntimeError> {
        Ok(self.refs.get(remote_ref).cloned())
    }

    fn read_commit_tree(&self, commit_oid: &str) -> Result<Vec<TreeObject>, RuntimeError> {
        *self.tree_reads.borrow_mut() += 1;
        self.trees
            .get(commit_oid)
            .cloned()
            .ok_or_else(|| RuntimeError::Provider("missing fixture tree".to_string()))
    }
}

fn evidence() -> SealedProviderEvidence {
    validate_provider_evidence(
        ProviderEvidenceInput {
            provider: ProviderKind::Diffinite,
            repository_id: "diffinite:thecartine/quilt".to_string(),
            policy_id: "hook-v8".to_string(),
            policy_sha256: digest(POLICY),
            integrator_id: "integrator".to_string(),
            refs: ProviderRefLayout::diffinite(),
            control_mode: ControlMode::IntegratorCompareAndSwap,
            canonical_mode: CanonicalMode::IntegratorFastForward,
            archives_create_only: true,
            submission_mode: SubmissionMode::CredentialInbox,
            exact_oid_reads: true,
        },
        POLICY,
        ProviderKind::Diffinite,
        "diffinite:thecartine/quilt",
    )
    .unwrap()
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
