use std::path::Path;
use std::process::Command;

use sha2::{Digest, Sha256};

use super::fence_local_store;
use crate::compaction::{
    CompactionManifest, Compatibility, GenerationState, Retention, SnapshotDescriptor, SnapshotSet,
    SourceCheckpoint, PROTOCOL_VERSION,
};
use crate::project::StorePaths;
use crate::sync::{GitAdapter, SyncError};

#[test]
fn pruning_removes_only_matching_cutoff_files_missing_from_retained_delta() {
    let fixture = fixture();
    let old = fixture.store.root.join("events/day/old.json");
    let unique = fixture.store.root.join("events/day/unique.json");
    std::fs::create_dir_all(old.parent().expect("event parent")).expect("event dir");
    std::fs::write(&old, b"old").expect("old local event");
    std::fs::write(&unique, b"unique").expect("unique local event");

    fence_local_store(
        &GitAdapter::new(),
        &fixture.repo,
        &fixture.store,
        &fixture.repo,
        &fixture.active_head,
    )
    .expect("validated pruning");

    assert!(
        !old.exists(),
        "matching compacted cutoff event should be pruned"
    );
    assert!(
        unique.exists(),
        "unpushed event absent from cutoff must survive"
    );
    assert_eq!(
        std::fs::read_to_string(
            fixture
                .store
                .root
                .join("compaction/local/active-generation")
        )
        .expect("marker"),
        format!("{}\n", fixture.generation_id)
    );
}

#[test]
fn pruning_preserves_retained_cutoff_files_and_is_idempotent() {
    let fixture = fixture();
    let retained = fixture.store.root.join("index/day/retained.json");
    std::fs::create_dir_all(retained.parent().expect("index parent")).expect("index dir");
    std::fs::write(&retained, b"retained").expect("retained local index");
    let git = GitAdapter::new();

    for _ in 0..2 {
        fence_local_store(
            &git,
            &fixture.repo,
            &fixture.store,
            &fixture.repo,
            &fixture.active_head,
        )
        .expect("idempotent prune");
    }
    assert!(retained.exists());
}

#[test]
fn pruning_rejects_conflicting_bytes_at_an_immutable_cutoff_path() {
    let fixture = fixture();
    let old = fixture.store.root.join("events/day/old.json");
    std::fs::create_dir_all(old.parent().expect("event parent")).expect("event dir");
    std::fs::write(&old, b"different").expect("conflicting local event");
    let error = fence_local_store(
        &GitAdapter::new(),
        &fixture.repo,
        &fixture.store,
        &fixture.repo,
        &fixture.active_head,
    )
    .expect_err("immutable conflict must fail");
    assert!(matches!(error, SyncError::FileConflict { .. }));
    assert!(old.exists(), "conflicting local bytes must not be deleted");
}

struct Fixture {
    _repo_ws: knots_test_support::TestWorkspace,
    _store_ws: knots_test_support::TestWorkspace,
    repo: std::path::PathBuf,
    store: StorePaths,
    active_head: String,
    generation_id: String,
}

fn fixture() -> Fixture {
    let repo_ws = knots_test_support::workspace("knots-generation-fence-repo");
    let store_ws = knots_test_support::workspace("knots-generation-fence-store");
    let repo = repo_ws.path().to_path_buf();
    git(&repo, &["init"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    write(&repo.join(".knots/events/day/old.json"), b"old");
    write(&repo.join(".knots/index/day/retained.json"), b"retained");
    git(&repo, &["add", "-f", ".knots"]);
    git(&repo, &["commit", "-m", "cutoff"]);
    let cutoff = output(&repo, &["rev-parse", "HEAD"]);
    let event_tree = output(&repo, &["rev-parse", "HEAD:.knots/events"]);
    let index_tree = output(&repo, &["rev-parse", "HEAD:.knots/index"]);

    std::fs::remove_file(repo.join(".knots/events/day/old.json")).expect("compact old event");
    let active_bytes = br#"{"schema_version":1,"hot":[],"warm":[]}"#;
    let cold_bytes = br#"{"schema_version":1,"cold":[]}"#;
    let active_path = ".knots/snapshots/active.snapshot.json";
    let cold_path = ".knots/snapshots/cold.snapshot.json";
    write(&repo.join(active_path), active_bytes);
    write(&repo.join(cold_path), cold_bytes);
    let manifest = CompactionManifest {
        protocol_version: PROTOCOL_VERSION,
        generation_id: String::new(),
        state: GenerationState::Active,
        source: SourceCheckpoint {
            remote_ref: "refs/heads/knots".to_string(),
            cutoff_commit: cutoff,
            index_tree,
            event_tree,
        },
        snapshots: SnapshotSet {
            active: descriptor(active_path, active_bytes),
            cold: descriptor(cold_path, cold_bytes),
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
    let generation_id = manifest.generation_id.clone();
    write(
        &repo
            .join(".knots/compaction/generations")
            .join(&generation_id)
            .join("manifest.json"),
        &serde_json::to_vec_pretty(&manifest).expect("manifest json"),
    );
    git(&repo, &["add", "-f", ".knots"]);
    git(&repo, &["commit", "-m", "active generation"]);
    let active_head = output(&repo, &["rev-parse", "HEAD"]);
    let store = StorePaths {
        root: store_ws.path().join(".knots"),
    };
    Fixture {
        _repo_ws: repo_ws,
        _store_ws: store_ws,
        repo,
        store,
        active_head,
        generation_id,
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

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
    std::fs::write(path, bytes).expect("write fixture");
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
