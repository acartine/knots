use std::path::Path;
use std::process::Command;

use super::{
    activate_generation, generation_ref, load_existing_manifest, read_active_events,
    reject_duplicate_events, validate_active_generation, verify_cleanup_events,
    DIFFINITE_GENERATION_TWO_REMOTE_REF,
};
use crate::compaction::{fixture_pack, RawEvent};
use crate::db;
use crate::sync_ref::{SyncRefConfig, GENERATION_TWO_REMOTE_REF};

#[test]
fn active_compaction_error_messages_cover_wrapped_failures() {
    let json = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
    let errors = [
        super::ActiveCompactionError::Json(json),
        super::ActiveCompactionError::Snapshot(crate::snapshots::SnapshotError::Io(
            std::io::Error::other("snapshot"),
        )),
        super::ActiveCompactionError::Sync(crate::sync::SyncError::GitUnavailable),
        super::ActiveCompactionError::Pack(crate::compaction::PackError::DuplicateEvent),
        super::ActiveCompactionError::Runtime(crate::compaction::RuntimeError::InvalidObject(
            "object",
        )),
    ];
    for error in errors {
        assert!(error.to_string().starts_with("compaction"));
    }
}

#[test]
fn active_compaction_rejects_unsafe_inputs() {
    let event = RawEvent {
        path: ".knots/events/event-1.json".to_string(),
        event_id: "event-1".to_string(),
        bytes: event_bytes("event-1"),
    };
    let mut packed = fixture_pack().descriptor;
    packed.events[0].path = event.path.clone();
    assert!(reject_duplicate_events(std::slice::from_ref(&event), &[packed]).is_err());

    let mut changed = event.clone();
    changed.bytes.push(b' ');
    assert!(verify_cleanup_events(&[changed], &[event]).is_err());
    assert_eq!(
        generation_ref("refs/work/example"),
        DIFFINITE_GENERATION_TWO_REMOTE_REF
    );

    let workspace = knots_test_support::workspace("active-compaction-invalid");
    let store = workspace.path().join(".knots");
    assert!(load_existing_manifest(&store).unwrap().is_none());
    write_event(&store.join("events/missing.json"), "missing");
    std::fs::write(store.join("events/missing.json"), b"{}").unwrap();
    assert!(read_active_events(&store).is_err());

    let checkout = workspace.path().join("checkout");
    let manifest_path = checkout.join(".knots/v2/active-generation.json");
    std::fs::create_dir_all(manifest_path.parent().unwrap()).unwrap();
    std::fs::write(
        manifest_path,
        br#"{"protocol_version":1,"source_ref":"knots","source_commit":"bad","snapshots":[],"packs":[]}"#,
    )
    .unwrap();
    assert!(validate_active_generation(&store, &checkout).is_err());
}

#[test]
fn activation_replaces_active_files_with_a_lossless_generation() {
    let repo_ws = knots_test_support::workspace("active-compaction-repo");
    let remote_ws = knots_test_support::workspace("active-compaction-remote");
    let repo = repo_ws.path();
    let remote = remote_ws.path();
    git(remote, &["init", "--bare"]);
    git(repo, &["init"]);
    git(repo, &["config", "user.name", "Compaction Test"]);
    git(repo, &["config", "user.email", "compaction@test.invalid"]);
    git(repo, &["commit", "--allow-empty", "-m", "initial"]);
    git(repo, &["remote", "add", "origin", path_text(remote)]);
    git(repo, &["push", "origin", "HEAD:refs/heads/knots"]);

    let store = repo.join(".knots");
    write_event(
        &store.join("v2/activation-quarantine/interrupted/events/2026/08/28/full.json"),
        "full-1",
    );
    write_event(&store.join("index/2026/08/28/head.json"), "head-1");
    write_event(&store.join("events/2026/08/28/full.json"), "full-1");
    write_event(
        &store.join("_worktree/.knots/events/2026/08/28/full.json"),
        "full-1",
    );
    write_event(
        &store.join("_worktree/.knots/index/2026/08/28/head.json"),
        "head-1",
    );
    git(repo, &["add", ".knots/events", ".knots/index"]);
    git(repo, &["commit", "-m", "publish fixture events"]);
    git(repo, &["push", "origin", "HEAD:refs/heads/knots"]);
    let db_path = store.join("cache/state.sqlite");
    std::fs::create_dir_all(db_path.parent().expect("database parent")).unwrap();
    let conn = db::open_connection(&db_path.to_string_lossy()).unwrap();

    let source = output(repo, &["rev-parse", "HEAD"]);
    let summary =
        activate_generation(&conn, repo, &store, source.trim()).expect("activate generation");
    assert_eq!(summary.legacy_files, 2);
    assert_eq!(summary.retained_packs, 1);
    assert!(!store.join("events").exists());
    assert!(!store.join("index").exists());
    assert!(!store.join("_worktree/.knots/events").exists());
    assert!(!store.join("_worktree/.knots/index").exists());
    assert!(!store.join("v2/activation-quarantine").exists());
    assert_eq!(
        SyncRefConfig::for_repo(repo).remote_ref(),
        GENERATION_TWO_REMOTE_REF
    );
    let manifest = load_existing_manifest(&store)
        .unwrap()
        .expect("local manifest");
    assert_eq!(manifest.packs[0].events.len(), 2);
    let remote_oid = output(
        repo,
        &["ls-remote", "--refs", "origin", GENERATION_TWO_REMOTE_REF],
    );
    assert!(remote_oid.starts_with(&summary.compacted_commit));
    let tree = output(
        repo,
        &["ls-tree", "-r", "--name-only", &summary.compacted_commit],
    );
    assert_eq!(tree.lines().count(), 4);
    assert!(!tree.contains(".knots/events/"));
    assert!(!tree.contains(".knots/index/"));

    verify_repeat_compaction(repo, &store, &conn, source.trim(), &summary, &manifest);
}

fn verify_repeat_compaction(
    repo: &Path,
    store: &Path,
    conn: &rusqlite::Connection,
    source: &str,
    summary: &super::ActiveCompactionSummary,
    manifest: &super::ActiveGenerationManifest,
) {
    git(repo, &["config", "knots.remoteRef", "refs/heads/knots"]);
    write_event(
        &store.join("_worktree/.knots/events/2026/08/28/full.json"),
        "full-1",
    );
    write_event(
        &store.join("_worktree/.knots/index/2026/08/28/head.json"),
        "head-1",
    );
    write_event(&store.join("events/2026/08/28/full.json"), "full-1");
    write_event(&store.join("index/2026/08/28/head.json"), "head-1");
    let resumed =
        activate_generation(conn, repo, store, source.trim()).expect("resume published generation");
    assert_eq!(resumed.compacted_commit, summary.compacted_commit);
    assert!(!store.join("events").exists());
    assert!(!store.join("index").exists());

    verify_activated_recovery(repo, store, &summary.compacted_commit, manifest);

    write_event(&store.join("events/2026/08/28/delta.json"), "full-2");
    write_event(
        &store.join("_worktree/.knots/events/2026/08/28/delta.json"),
        "full-2",
    );
    let mut source_files = super::generation_files(store, manifest).unwrap();
    source_files.insert(
        ".knots/events/2026/08/28/delta.json".to_string(),
        event_bytes("full-2"),
    );
    let provider = crate::compaction::GitObjectProvider::new(repo);
    let source_v2 = provider
        .write_commit(
            source_files,
            Some(&summary.compacted_commit),
            "knots: publish delta fixture\n",
        )
        .unwrap();
    git(
        repo,
        &[
            "push",
            "origin",
            &format!("{source_v2}:refs/heads/knots-v2"),
        ],
    );
    let repeated = activate_generation(conn, repo, store, &source_v2).expect("repeat compaction");
    assert_eq!(repeated.legacy_files, 1);
    assert_eq!(repeated.retained_packs, 2);
    let repeated_manifest = load_existing_manifest(store)
        .unwrap()
        .expect("repeated manifest");
    assert_eq!(repeated_manifest.packs.len(), 2);
    assert!(!store.join("events").exists());
}

fn verify_activated_recovery(
    repo: &Path,
    store: &Path,
    commit: &str,
    manifest: &super::ActiveGenerationManifest,
) {
    let checkout_ws = knots_test_support::workspace("active-compaction-checkout");
    let checkout = checkout_ws.path();
    let checkout_text = path_text(checkout);
    git(
        repo,
        &[
            "--work-tree",
            checkout_text,
            "checkout",
            commit,
            "--",
            ".knots",
        ],
    );
    let wrong = store.join("v2/activation-quarantine/interrupted/events/full.json");
    write_event(&wrong, "full-1");
    std::fs::remove_file(store.join("v2/active-generation.json")).unwrap();
    for pack in &manifest.packs {
        std::fs::remove_file(store.join(pack.path.trim_start_matches(".knots/"))).unwrap();
    }

    let wrong_path = validate_active_generation(store, checkout).unwrap_err();
    assert!(wrong_path
        .to_string()
        .contains("absent from the activated pack"));
    assert!(wrong.exists());
    std::fs::remove_file(&wrong).unwrap();
    let quarantine = store.join("v2/activation-quarantine/interrupted/events/2026/08/28/full.json");
    write_event(&quarantine, "full-1");
    validate_active_generation(store, checkout).expect("validate and finish activation");
    assert!(!store.join("v2/activation-quarantine").exists());
    assert!(store.join("v2/active-generation.json").exists());
    for pack in &manifest.packs {
        assert!(store.join(pack.path.trim_start_matches(".knots/")).exists());
    }

    let snapshot = &manifest.snapshots[0];
    std::fs::write(checkout.join(&snapshot.path), b"corrupted").unwrap();
    let error = validate_active_generation(store, checkout).unwrap_err();
    assert!(error.to_string().contains("failed validation"));
}

fn write_event(path: &Path, event_id: &str) {
    std::fs::create_dir_all(path.parent().expect("event parent")).unwrap();
    std::fs::write(path, event_bytes(event_id)).unwrap();
}

fn event_bytes(event_id: &str) -> Vec<u8> {
    let body = serde_json::json!({"event_id": event_id, "body": "exact bytes"});
    serde_json::to_vec_pretty(&body).unwrap()
}

fn git(cwd: &Path, args: &[&str]) {
    let result = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn output(cwd: &Path, args: &[&str]) -> String {
    let result = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(result.status.success());
    String::from_utf8(result.stdout).unwrap()
}

fn path_text(path: &Path) -> &str {
    path.to_str().expect("UTF-8 fixture path")
}
