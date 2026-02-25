use std::error::Error;
use std::path::PathBuf;

use super::SyncError;

#[test]
fn sync_error_classifiers_detect_expected_git_failures() {
    let missing_remote = SyncError::GitCommandFailed {
        command: "git fetch origin".to_string(),
        code: Some(128),
        stderr: "fatal: No such remote 'origin'".to_string(),
    };
    assert!(missing_remote.is_missing_remote());
    assert!(!missing_remote.is_unknown_revision());
    assert!(!missing_remote.is_non_fast_forward());

    let unknown_revision = SyncError::GitCommandFailed {
        command: "git rev-parse bad".to_string(),
        code: Some(128),
        stderr: "fatal: bad object deadbeef".to_string(),
    };
    assert!(!unknown_revision.is_missing_remote());
    assert!(unknown_revision.is_unknown_revision());
    assert!(!unknown_revision.is_non_fast_forward());

    let non_fast_forward = SyncError::GitCommandFailed {
        command: "git push".to_string(),
        code: Some(1),
        stderr: "rejected non-fast-forward".to_string(),
    };
    assert!(!non_fast_forward.is_missing_remote());
    assert!(!non_fast_forward.is_unknown_revision());
    assert!(non_fast_forward.is_non_fast_forward());
}

#[test]
fn sync_error_display_source_and_from_cover_all_variants() {
    let io: SyncError = std::io::Error::other("disk").into();
    assert!(io.to_string().contains("I/O error"));
    assert!(io.source().is_some());

    let db: SyncError = rusqlite::Error::InvalidQuery.into();
    assert!(db.to_string().contains("database error"));
    assert!(db.source().is_some());

    let unavailable = SyncError::GitUnavailable;
    assert!(unavailable.to_string().contains("git CLI is not installed"));
    assert!(unavailable.source().is_none());

    let command_failed = SyncError::GitCommandFailed {
        command: "git fetch".to_string(),
        code: Some(1),
        stderr: "bad".to_string(),
    };
    assert!(command_failed.to_string().contains("git command failed"));
    assert!(command_failed.source().is_none());

    let dirty = SyncError::DirtyWorktree(PathBuf::from("/tmp/worktree"));
    assert!(dirty.to_string().contains("has uncommitted changes"));
    assert!(dirty.source().is_none());

    let invalid_event = SyncError::InvalidEvent {
        path: PathBuf::from("/tmp/event.json"),
        message: "bad payload".to_string(),
    };
    assert!(invalid_event.to_string().contains("invalid event"));
    assert!(invalid_event.source().is_none());

    let conflict = SyncError::FileConflict {
        path: PathBuf::from("/tmp/file.json"),
    };
    assert!(conflict.to_string().contains("push conflict"));
    assert!(conflict.source().is_none());

    let escalation = SyncError::MergeConflictEscalation {
        message: "needs manual merge".to_string(),
    };
    assert!(escalation.to_string().contains("merge conflict escalation"));
    assert!(escalation.source().is_none());

    let snapshot = SyncError::SnapshotLoad {
        message: "missing snapshot".to_string(),
    };
    assert!(snapshot.to_string().contains("snapshot load failed"));
    assert!(snapshot.source().is_none());
}
