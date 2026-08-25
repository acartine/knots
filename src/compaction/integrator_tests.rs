use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::*;

const OBJECT_A: &str = "1111111111111111111111111111111111111111";
const OBJECT_B: &str = "2222222222222222222222222222222222222222";
const TREE_A: &str = "3333333333333333333333333333333333333333";
const TREE_B: &str = "4444444444444444444444444444444444444444";

struct Replay {
    mismatch: bool,
}

impl ProjectionReplay for Replay {
    fn project(
        &self,
        _predecessor: &GenerationBase,
        accepted: &[RawEvent],
        _conflicts: &[ConflictRecord],
    ) -> Result<CanonicalProjections, String> {
        Ok(projections(accepted, false))
    }

    fn full_replay(
        &self,
        _predecessor: &GenerationBase,
        accepted: &[RawEvent],
        _conflicts: &[ConflictRecord],
    ) -> Result<CanonicalProjections, String> {
        Ok(projections(accepted, self.mismatch))
    }
}

fn projections(events: &[RawEvent], mismatch: bool) -> CanonicalProjections {
    let ids = events
        .iter()
        .map(|event| Value::String(event.event_id.clone()))
        .collect::<Vec<_>>();
    CanonicalProjections {
        schema_version: 1,
        hot: vec![json!({"event_ids": ids, "mismatch": mismatch})],
        warm: vec![json!({"id": "warm"})],
        cold: vec![json!({"id": "cold"})],
        edges: vec![json!({"src": "a", "dst": "b"})],
        leases: vec![json!({"id": "lease"})],
        workflows: vec![json!({"id": "work_sdlc"})],
        metadata: vec![json!({"id": "note"})],
        conflicts: Vec::new(),
    }
}

fn candidate(
    writer: &str,
    sequence: u64,
    previous: Option<&str>,
    base: Option<&str>,
    commit: &str,
    event_id: &str,
) -> InboxCandidate {
    let path = format!("{INBOX_DATA_ROOT}/events/{event_id}.json");
    let bytes = serde_json::to_vec(&json!({
        "event_id": event_id,
        "occurred_at": "2026-08-25T20:00:00Z",
        "knot_id": format!("knot-{event_id}"),
        "type": "knot.created",
        "data": {"title": event_id}
    }))
    .expect("event serializes");
    let bundle = InboxBundle {
        schema_version: 1,
        writer_id: writer.to_string(),
        sequence,
        previous_head: previous.map(str::to_string),
        base_generation: base.map(str::to_string),
        entries: vec![InboxEntry {
            path: path.clone(),
            event_id: event_id.to_string(),
            content_sha256: digest(&bytes),
            bytes: bytes.len() as u64,
        }],
    };
    InboxCandidate {
        inbox_ref: V2RefLayout::default().inbox(writer),
        expected_head: commit.to_string(),
        observed_head: commit.to_string(),
        bundle: serde_json::to_vec(&bundle).expect("bundle serializes"),
        objects: BTreeMap::from([(path, bytes)]),
    }
}

fn input(base: GenerationBase, inboxes: Vec<InboxCandidate>) -> GenerationInput {
    let active = active_bytes();
    let cold = cold_bytes();
    GenerationInput {
        base,
        source: SourceCheckpoint {
            legacy_ref: LEGACY_REF.to_string(),
            cutoff_commit: OBJECT_A.to_string(),
            index_tree: TREE_A.to_string(),
            event_tree: TREE_B.to_string(),
        },
        snapshots: SnapshotSet {
            active: descriptor(
                ".knots/v2/generations/current/active.snapshot.json",
                &active,
                0,
            ),
            cold: descriptor(".knots/v2/generations/current/cold.snapshot.json", &cold, 0),
        },
        active_snapshot: active,
        cold_snapshot: cold,
        inboxes,
        action: ControlKind::Activation,
    }
}

fn empty_base() -> GenerationBase {
    GenerationBase {
        generation_id: None,
        control_epoch: 0,
        control_head: None,
        acknowledged_writer_heads: Vec::new(),
    }
}

fn active_bytes() -> Vec<u8> {
    br#"{"schema_version":1,"hot":[],"warm":[]}"#.to_vec()
}

fn cold_bytes() -> Vec<u8> {
    br#"{"schema_version":1,"cold":[]}"#.to_vec()
}

fn descriptor(path: &str, bytes: &[u8], records: u64) -> SnapshotDescriptor {
    SnapshotDescriptor {
        path: path.to_string(),
        sha256: digest(bytes),
        bytes: bytes.len() as u64,
        records,
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn integrator_is_deterministic_projection_complete_and_lossless() {
    let a = candidate("writer-a", 1, None, None, OBJECT_A, "event-a");
    let b = candidate("writer-b", 1, None, None, OBJECT_B, "event-b");
    let left = integrate_generation(
        input(empty_base(), vec![a.clone(), b.clone()]),
        &Replay { mismatch: false },
        OBJECT_A,
        &digest(b"policy"),
    )
    .expect("generation integrates");
    let right = integrate_generation(
        input(empty_base(), vec![b, a]),
        &Replay { mismatch: false },
        OBJECT_A,
        &digest(b"policy"),
    )
    .expect("input order is irrelevant");
    assert_eq!(left.manifest, right.manifest);
    assert_eq!(left.pack.compressed, right.pack.compressed);
    assert_eq!(left.manifest.writer_heads.len(), 2);
    let value: Value = serde_json::from_slice(&left.projections).expect("projection json");
    for field in [
        "hot",
        "warm",
        "cold",
        "edges",
        "leases",
        "workflows",
        "metadata",
        "conflicts",
    ] {
        assert!(value.get(field).is_some(), "missing {field}");
    }
    let decoded = zstd::stream::decode_all(left.pack.compressed.as_slice()).expect("pack decodes");
    for event in &left.pack.descriptor.events {
        let raw = &decoded[event.offset as usize..(event.offset + event.bytes) as usize];
        assert_eq!(digest(raw), event.content_sha256);
    }
}

#[test]
fn inbox_validation_rejects_heads_paths_hashes_and_unknown_schema() {
    let valid = candidate("writer-a", 1, None, None, OBJECT_A, "event-a");
    assert_eq!(validate_inbox(&valid).expect("valid").events.len(), 1);

    let mut wrong_head = valid.clone();
    wrong_head.expected_head = OBJECT_B.to_string();
    assert_eq!(validate_inbox(&wrong_head), Err(InboxError::InvalidHead));

    let mut wrong_path = valid.clone();
    let bytes = wrong_path.objects.pop_first().expect("object").1;
    let path = ".github/workflows/pwn.yml".to_string();
    wrong_path.objects.insert(path.clone(), bytes);
    let mut bundle: InboxBundle = serde_json::from_slice(&wrong_path.bundle).unwrap();
    bundle.entries[0].path = path;
    wrong_path.bundle = serde_json::to_vec(&bundle).unwrap();
    assert_eq!(validate_inbox(&wrong_path), Err(InboxError::InvalidPath));

    let mut wrong_hash = valid.clone();
    let mut bundle: InboxBundle = serde_json::from_slice(&wrong_hash.bundle).unwrap();
    bundle.entries[0].content_sha256 = digest(b"different");
    wrong_hash.bundle = serde_json::to_vec(&bundle).unwrap();
    assert_eq!(validate_inbox(&wrong_hash), Err(InboxError::HashMismatch));

    let mut value = serde_json::to_value(bundle).unwrap();
    value["execute"] = json!(true);
    wrong_hash.bundle = serde_json::to_vec(&value).unwrap();
    assert_eq!(validate_inbox(&wrong_hash), Err(InboxError::InvalidBundle));

    let mut invalid_id = valid;
    let mut bundle: InboxBundle = serde_json::from_slice(&invalid_id.bundle).unwrap();
    bundle.entries[0].event_id = "../event-a".to_string();
    invalid_id.bundle = serde_json::to_vec(&bundle).unwrap();
    assert_eq!(validate_inbox(&invalid_id), Err(InboxError::InvalidEventId));
}

#[test]
fn stale_bases_are_durably_conflicted_and_writer_chains_are_monotonic() {
    let prior = WriterHead {
        writer_id: "writer-a".to_string(),
        inbox_ref: V2RefLayout::default().inbox("writer-a"),
        commit: OBJECT_A.to_string(),
        sequence: 3,
    };
    let base = GenerationBase {
        generation_id: Some("v2-current".to_string()),
        control_epoch: 7,
        control_head: Some(OBJECT_B.to_string()),
        acknowledged_writer_heads: vec![prior.clone()],
    };
    let stale = candidate(
        "writer-a",
        4,
        Some(OBJECT_A),
        Some("v2-old"),
        OBJECT_B,
        "event-stale",
    );
    let result = integrate_generation(
        input(base.clone(), vec![stale]),
        &Replay { mismatch: false },
        OBJECT_A,
        &digest(b"policy"),
    )
    .expect("stale bundle is retained as a conflict");
    assert_eq!(result.conflicts.len(), 1);
    assert_eq!(result.control.acknowledged_writer_heads[0].sequence, 4);
    assert_eq!(result.pack.descriptor.events.len(), 1);
    let projection: Value = serde_json::from_slice(&result.projections).unwrap();
    assert_eq!(projection["conflicts"].as_array().unwrap().len(), 1);

    let overflow_base = GenerationBase {
        generation_id: Some("v2-current".to_string()),
        control_epoch: 7,
        control_head: Some(OBJECT_B.to_string()),
        acknowledged_writer_heads: vec![WriterHead {
            sequence: u64::MAX,
            ..prior
        }],
    };
    let overflow = candidate(
        "writer-a",
        u64::MAX,
        Some(OBJECT_A),
        Some("v2-current"),
        OBJECT_B,
        "event-overflow",
    );
    assert_eq!(
        integrate_generation(
            input(overflow_base, vec![overflow]),
            &Replay { mismatch: false },
            OBJECT_A,
            &digest(b"policy")
        ),
        Err(IntegrationError::WriterSequence)
    );

    let skipped = candidate(
        "writer-a",
        5,
        Some(OBJECT_A),
        Some("v2-current"),
        OBJECT_B,
        "event-skipped",
    );
    assert_eq!(
        integrate_generation(
            input(base, vec![skipped]),
            &Replay { mismatch: false },
            OBJECT_A,
            &digest(b"policy")
        ),
        Err(IntegrationError::WriterSequence)
    );
}

#[test]
fn replay_mismatch_and_invalid_recovery_fail_before_publication() {
    let candidate = candidate("writer-a", 1, None, None, OBJECT_A, "event-a");
    assert_eq!(
        integrate_generation(
            input(empty_base(), vec![candidate]),
            &Replay { mismatch: true },
            OBJECT_A,
            &digest(b"policy")
        ),
        Err(IntegrationError::ReplayMismatch)
    );

    let mut exhausted = empty_base();
    exhausted.control_epoch = u64::MAX;
    assert_eq!(
        integrate_generation(
            input(exhausted, Vec::new()),
            &Replay { mismatch: false },
            OBJECT_A,
            &digest(b"policy")
        ),
        Err(IntegrationError::ControlEpoch)
    );
    let mut recovery = input(empty_base(), Vec::new());
    recovery.action = ControlKind::Recovery {
        rollback_of_epoch: 0,
    };
    assert_eq!(
        integrate_generation(
            recovery,
            &Replay { mismatch: false },
            OBJECT_A,
            &digest(b"policy")
        ),
        Err(IntegrationError::InvalidRecovery)
    );
}

#[test]
fn concurrent_inbox_refs_have_one_initial_cas_winner_and_lossless_retry() {
    let root = knots_test_support::workspace("knots-v2-integrator-race");
    let remote = root.path().join("origin.git");
    let repo = root.path().join("repo");
    git(root.path(), &["init", "--bare", remote.to_str().unwrap()]);
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    git(
        &repo,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    let commit_a = commit(&repo, "a");
    push(
        &repo,
        &commit_a,
        &V2RefLayout::default().inbox("writer-a"),
        None,
    )
    .expect("writer a inbox");
    let commit_b = commit(&repo, "b");
    push(
        &repo,
        &commit_b,
        &V2RefLayout::default().inbox("writer-b"),
        None,
    )
    .expect("writer b inbox");

    let first = integrate_generation(
        input(
            empty_base(),
            vec![candidate("writer-a", 1, None, None, &commit_a, "event-a")],
        ),
        &Replay { mismatch: false },
        &commit_a,
        &digest(b"policy"),
    )
    .unwrap();
    let losing = integrate_generation(
        input(
            empty_base(),
            vec![candidate("writer-b", 1, None, None, &commit_b, "event-b")],
        ),
        &Replay { mismatch: false },
        &commit_b,
        &digest(b"policy"),
    )
    .unwrap();
    let control = V2RefLayout::default().control;
    push(&repo, &commit_a, control, None).expect("first CAS wins");
    assert!(push(&repo, &commit_b, control, None).is_err());

    let retry_base = GenerationBase {
        generation_id: Some(first.manifest.generation_id.clone()),
        control_epoch: first.control.epoch,
        control_head: Some(commit_a.clone()),
        acknowledged_writer_heads: first.control.acknowledged_writer_heads.clone(),
    };
    let retry = integrate_generation(
        input(
            retry_base,
            vec![candidate("writer-b", 1, None, None, &commit_b, "event-b")],
        ),
        &Replay { mismatch: false },
        &commit_b,
        &digest(b"policy"),
    )
    .expect("loser rebuilds from winning control");
    assert_eq!(losing.control.acknowledged_writer_heads.len(), 1);
    assert_eq!(retry.control.acknowledged_writer_heads.len(), 2);
    assert_eq!(retry.conflicts.len(), 1, "stale tail is retained, not lost");
    push(&repo, &commit_b, control, Some(&commit_a)).expect("retry CAS advances control");
    assert_eq!(git_output(&remote, &["rev-parse", control]), commit_b);
}

fn commit(repo: &Path, name: &str) -> String {
    std::fs::write(repo.join(name), name).unwrap();
    git(repo, &["add", name]);
    git(repo, &["commit", "-m", name]);
    git_output(repo, &["rev-parse", "HEAD"])
}

fn push(repo: &Path, source: &str, target: &str, expected: Option<&str>) -> Result<(), ()> {
    let lease = format!(
        "--force-with-lease={target}:{}",
        expected.unwrap_or_default()
    );
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["push", &lease, "origin", &format!("{source}:{target}")])
        .output()
        .unwrap();
    output.status.success().then_some(()).ok_or(())
}

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_output(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}
