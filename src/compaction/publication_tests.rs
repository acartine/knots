use std::path::Path;
use std::process::Command;

use sha2::{Digest, Sha256};

use super::{GenerationPublisher, PublicationTarget};
use crate::compaction::{
    build_pack, validate_protection, CompactionManifest, Compatibility, ProtectionMarker,
    ProviderProtectionFacts, RawEvent, SnapshotDescriptor, SnapshotSet, SourceCheckpoint,
    SourceFacts, V2RefLayout, ValidationContext, WriterHead, LEGACY_REF, PROTOCOL_VERSION,
};

const COMMIT: &str = "1111111111111111111111111111111111111111";
const INDEX_TREE: &str = "2222222222222222222222222222222222222222";
const EVENT_TREE: &str = "3333333333333333333333333333333333333333";

#[test]
fn immutable_generation_is_published_before_control_cas() {
    let remote_ws = knots_test_support::workspace("knots-v2-protected-remote");
    let repo_ws = knots_test_support::workspace("knots-v2-protected-publisher");
    let remote = remote_ws.path();
    let repo = repo_ws.path();
    git(remote, &["init", "--bare"]);
    git(repo, &["init"]);
    git(repo, &["config", "user.email", "test@example.com"]);
    git(repo, &["config", "user.name", "Test"]);
    std::fs::write(repo.join("seed"), "one").expect("write seed");
    git(repo, &["add", "seed"]);
    git(repo, &["commit", "-m", "seed"]);
    git(
        repo,
        &["remote", "add", "origin", &remote.display().to_string()],
    );
    let commit = output(repo, &["rev-parse", "HEAD"]);
    let (manifest, active, cold, projections, pack) = manifest();
    let pack_refs = [(manifest.packs[0].pack_id.as_str(), pack.as_slice())];
    let context = context(&active, &cold, &projections, &pack_refs);
    let protection = protection(None);
    let publisher = GenerationPublisher::new();

    publisher
        .publish_immutable(
            PublicationTarget {
                repo,
                remote: "origin",
                commit: &commit,
            },
            &manifest,
            &context,
            &protection,
        )
        .expect("immutable objects publish create-only");

    let refs = V2RefLayout::default();
    assert_eq!(
        output(
            remote,
            &["rev-parse", &refs.canonical(&manifest.generation_id)]
        ),
        commit
    );
    assert_eq!(
        output(
            remote,
            &["rev-parse", &refs.archive(&manifest.generation_id)]
        ),
        commit
    );
    assert!(!ref_exists(remote, refs.control));

    publisher
        .activate_control(
            PublicationTarget {
                repo,
                remote: "origin",
                commit: &commit,
            },
            None,
            &protection,
        )
        .expect("control CAS is last");
    assert_eq!(output(remote, &["rev-parse", refs.control]), commit);
}

#[test]
fn legacy_push_cannot_change_canonical_v2_ref() {
    let remote_ws = knots_test_support::workspace("knots-v2-legacy-isolation-remote");
    let repo_ws = knots_test_support::workspace("knots-v2-legacy-isolation-repo");
    let remote = remote_ws.path();
    let repo = repo_ws.path();
    git(remote, &["init", "--bare"]);
    git(repo, &["init"]);
    git(repo, &["config", "user.email", "test@example.com"]);
    git(repo, &["config", "user.name", "Test"]);
    std::fs::write(repo.join("seed"), "canonical").expect("seed canonical");
    git(repo, &["add", "seed"]);
    git(repo, &["commit", "-m", "canonical"]);
    git(
        repo,
        &["remote", "add", "origin", &remote.display().to_string()],
    );
    let canonical_ref = V2RefLayout::default().canonical("fixture");
    git(repo, &["push", "origin", &format!("HEAD:{canonical_ref}")]);
    let before = output(remote, &["rev-parse", &canonical_ref]);

    std::fs::write(repo.join("legacy"), "v0.17.6 event").expect("legacy event");
    git(repo, &["add", "legacy"]);
    git(repo, &["commit", "-m", "knots: publish local events"]);
    git(repo, &["push", "origin", &format!("HEAD:{LEGACY_REF}")]);
    assert_eq!(output(remote, &["rev-parse", &canonical_ref]), before);
    assert_ne!(output(remote, &["rev-parse", LEGACY_REF]), before);
}

#[test]
fn activation_rejects_stale_provider_head() {
    let publisher = GenerationPublisher::new();
    let ws = knots_test_support::workspace("knots-v2-activation-rejections");
    let target = PublicationTarget {
        repo: ws.path(),
        remote: "origin",
        commit: COMMIT,
    };
    let protected = protection(Some(COMMIT));
    let error = publisher
        .activate_control(target, None, &protected)
        .expect_err("stale provider head fails before Git");
    assert!(error.to_string().contains("provider control head"));
}

fn manifest() -> (CompactionManifest, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>) {
    let active = br#"{"schema_version":1,"hot":[],"warm":[]}"#.to_vec();
    let cold = br#"{"schema_version":1,"cold":[]}"#.to_vec();
    let projections = br#"{"schema_version":1,"hot":[],"warm":[],"cold":[],"edges":[],"leases":[],"workflows":[],"metadata":[],"conflicts":[]}"#.to_vec();
    let pack = build_pack(&[RawEvent {
        path: ".knots/v2/inbox/events/event-1.json".to_string(),
        event_id: "event-1".to_string(),
        bytes: br#"{"event_id":"event-1"}"#.to_vec(),
    }])
    .expect("pack builds");
    let value = CompactionManifest {
        protocol_version: PROTOCOL_VERSION,
        generation_id: String::new(),
        predecessor_generation: None,
        predecessor_control_epoch: 0,
        source: SourceCheckpoint {
            legacy_ref: LEGACY_REF.to_string(),
            cutoff_commit: COMMIT.to_string(),
            index_tree: INDEX_TREE.to_string(),
            event_tree: EVENT_TREE.to_string(),
        },
        writer_heads: vec![WriterHead {
            writer_id: "writer-a".to_string(),
            inbox_ref: V2RefLayout::default().inbox("writer-a"),
            commit: COMMIT.to_string(),
            sequence: 1,
        }],
        snapshots: SnapshotSet {
            active: descriptor(
                ".knots/v2/generations/current/active.snapshot.json",
                &active,
            ),
            cold: descriptor(".knots/v2/generations/current/cold.snapshot.json", &cold),
        },
        projections: descriptor(
            ".knots/v2/generations/current/state.projections.json",
            &projections,
        ),
        packs: vec![pack.descriptor],
        compatibility: Compatibility {
            minimum_reader_protocol: PROTOCOL_VERSION,
            minimum_writer_protocol: PROTOCOL_VERSION,
        },
    }
    .seal();
    (value, active, cold, projections, pack.compressed)
}

fn descriptor(path: &str, bytes: &[u8]) -> SnapshotDescriptor {
    SnapshotDescriptor {
        path: path.to_string(),
        sha256: digest(bytes),
        bytes: bytes.len() as u64,
        records: 0,
    }
}

fn context<'a>(
    active: &'a [u8],
    cold: &'a [u8],
    projections: &'a [u8],
    packs: &'a [(&'a str, &'a [u8])],
) -> ValidationContext<'a> {
    ValidationContext {
        active_snapshot: Some(active),
        cold_snapshot: Some(cold),
        projections: Some(projections),
        packs,
        source: SourceFacts {
            cutoff_resolves: true,
            cutoff_is_ancestor: true,
            index_tree: Some(INDEX_TREE),
            event_tree: Some(EVENT_TREE),
        },
        expected_predecessor: None,
        expected_control_epoch: 0,
    }
}

fn protection(control_head: Option<&str>) -> crate::compaction::ValidatedProtection {
    let policy = b"enforced provider policy";
    let refs = V2RefLayout::default();
    let marker = ProtectionMarker {
        schema_version: 1,
        repository_id: "repo-1".to_string(),
        policy_id: "policy-1".to_string(),
        policy_sha256: digest(policy),
        integrator_id: "integrator".to_string(),
        control_ref: refs.control.to_string(),
        canonical_prefix: refs.canonical_prefix.to_string(),
        archive_prefix: refs.archive_prefix.to_string(),
        inbox_prefix: refs.inbox_prefix.to_string(),
    };
    let facts = ProviderProtectionFacts {
        repository_id: "repo-1",
        policy_id: "policy-1",
        policy_bytes: policy,
        integrator_id: "integrator",
        control_ref: refs.control,
        canonical_prefix: refs.canonical_prefix,
        archive_prefix: refs.archive_prefix,
        inbox_prefix: refs.inbox_prefix,
        control_head,
        control_protected: true,
        canonical_create_only: true,
        archives_create_only: true,
        inboxes_writer_scoped: true,
    };
    validate_protection(Some(&marker), Some(&facts)).expect("valid protection")
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn ref_exists(repo: &Path, reference: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "--verify", reference])
        .output()
        .expect("run git")
        .status
        .success()
}

fn git(repo: &Path, args: &[&str]) {
    let result = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        result.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn output(repo: &Path, args: &[&str]) -> String {
    let result = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("run git");
    assert!(result.status.success(), "git {args:?} failed");
    String::from_utf8_lossy(&result.stdout).trim().to_string()
}
