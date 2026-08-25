use std::path::Path;
use std::process::Command;

use sha2::{Digest, Sha256};

use super::{GenerationPublisher, PublicationTarget};
use crate::compaction::{
    CompactionManifest, Compatibility, GenerationState, Retention, SnapshotDescriptor, SnapshotSet,
    SourceCheckpoint, SourceFacts, ValidationContext, PROTOCOL_VERSION,
};

const ACTIVE_REF: &str = "refs/heads/knots-v2";
const LEGACY_REF: &str = "refs/heads/knots";

#[test]
fn staging_and_activation_are_create_only_compare_and_swap() {
    let remote_ws = knots_test_support::workspace("knots-generation-remote");
    let repo_ws = knots_test_support::workspace("knots-generation-publisher");
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

    let (prepared, active, active_bytes, cold_bytes) = manifests();
    let prepared_context = context(&prepared, &active_bytes, &cold_bytes);
    let publisher = GenerationPublisher::new();
    let staging_ref = format!("refs/heads/knots-v2-staging/{}", prepared.generation_id);
    publisher
        .stage(
            target(repo, &staging_ref, &commit),
            &prepared,
            &prepared_context,
        )
        .expect("first staging publication wins");
    std::fs::write(repo.join("racer"), "two").expect("write racing commit");
    git(repo, &["add", "racer"]);
    git(repo, &["commit", "-m", "racing publication"]);
    let competing_commit = output(repo, &["rev-parse", "HEAD"]);
    assert!(publisher
        .stage(
            target(repo, &staging_ref, &competing_commit),
            &prepared,
            &prepared_context,
        )
        .is_err());

    let active_context = context(&active, &active_bytes, &cold_bytes);
    publisher
        .activate(
            target(repo, ACTIVE_REF, &competing_commit),
            None,
            &active,
            &active_context,
        )
        .expect("first activation wins");
    assert!(publisher
        .activate(
            target(repo, ACTIVE_REF, &commit),
            None,
            &active,
            &active_context,
        )
        .is_err());
}

#[test]
fn legacy_writes_cannot_move_the_generation_two_ref() {
    let remote_ws = knots_test_support::workspace("knots-generation-isolation-remote");
    let repo_ws = knots_test_support::workspace("knots-generation-isolation-repo");
    let remote = remote_ws.path();
    let repo = repo_ws.path();
    git(remote, &["init", "--bare"]);
    git(repo, &["init"]);
    git(repo, &["config", "user.email", "test@example.com"]);
    git(repo, &["config", "user.name", "Test"]);
    std::fs::write(repo.join("seed"), "one").expect("write seed");
    git(repo, &["add", "seed"]);
    git(repo, &["commit", "-m", "first"]);
    git(
        repo,
        &["remote", "add", "origin", &remote.display().to_string()],
    );
    git(repo, &["push", "origin", &format!("HEAD:{ACTIVE_REF}")]);
    let active = output(remote, &["rev-parse", ACTIVE_REF]);

    std::fs::write(repo.join("seed"), "two").expect("advance legacy");
    git(repo, &["commit", "-am", "legacy write"]);
    git(repo, &["push", "origin", &format!("HEAD:{LEGACY_REF}")]);
    assert_eq!(output(remote, &["rev-parse", ACTIVE_REF]), active);
    assert_ne!(output(remote, &["rev-parse", LEGACY_REF]), active);
}

fn manifests() -> (CompactionManifest, CompactionManifest, Vec<u8>, Vec<u8>) {
    let active_bytes = br#"{"schema_version":1,"hot":[],"warm":[]}"#.to_vec();
    let cold_bytes = br#"{"schema_version":1,"cold":[]}"#.to_vec();
    let manifest = CompactionManifest {
        protocol_version: PROTOCOL_VERSION,
        generation_id: String::new(),
        state: GenerationState::Prepared,
        source: SourceCheckpoint {
            remote_ref: LEGACY_REF.to_string(),
            cutoff_commit: "a".repeat(40),
            index_tree: "b".repeat(40),
            event_tree: "c".repeat(40),
        },
        snapshots: SnapshotSet {
            active: descriptor(".knots/snapshots/active.snapshot.json", &active_bytes),
            cold: descriptor(".knots/snapshots/cold.snapshot.json", &cold_bytes),
        },
        retention: Retention {
            max_full_files: 1000,
            max_index_files: 1000,
        },
        compatibility: Compatibility {
            minimum_reader_protocol: PROTOCOL_VERSION,
            minimum_writer_protocol: PROTOCOL_VERSION,
        },
        previous_generation: None,
    }
    .seal();
    let active = CompactionManifest {
        state: GenerationState::Active,
        ..manifest.clone()
    };
    (manifest, active, active_bytes, cold_bytes)
}

fn target<'a>(repo: &'a Path, remote_ref: &'a str, commit: &'a str) -> PublicationTarget<'a> {
    PublicationTarget {
        repo,
        remote: "origin",
        remote_ref,
        commit,
    }
}

fn descriptor(path: &str, bytes: &[u8]) -> SnapshotDescriptor {
    SnapshotDescriptor {
        path: path.to_string(),
        sha256: format!("{:x}", Sha256::digest(bytes)),
        bytes: bytes.len() as u64,
        records: 0,
    }
}

fn context<'a>(
    manifest: &'a CompactionManifest,
    active: &'a [u8],
    cold: &'a [u8],
) -> ValidationContext<'a> {
    ValidationContext {
        active_snapshot: Some(active),
        cold_snapshot: Some(cold),
        source: SourceFacts {
            cutoff_resolves: true,
            cutoff_is_ancestor: true,
            index_tree: Some(&manifest.source.index_tree),
            event_tree: Some(&manifest.source.event_tree),
        },
        expected_predecessor: None,
        predecessor_chain: &[],
    }
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
