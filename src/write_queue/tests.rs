use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use crate::locks::{FileLock, LockError};

use super::*;

fn unique_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-queue-test-{}", uuid::Uuid::now_v7()));
    fs::create_dir_all(&root).expect("temp root should be creatable");
    root
}

fn state_request(
    root: &Path,
    paths: &QueuePaths,
    request_id: &str,
    knot_id: &str,
) -> QueuedWriteRequest {
    QueuedWriteRequest {
        request_id: request_id.to_string(),
        repo_root: root.display().to_string(),
        store_root: root.join(".knots").display().to_string(),
        distribution: "git".to_string(),
        project_id: None,
        db_path: root.join(".knots/cache/state.sqlite").display().to_string(),
        response_path: paths
            .responses_dir
            .join(format!("{request_id}.json"))
            .display()
            .to_string(),
        operation: WriteOperation::State(StateOperation {
            id: knot_id.to_string(),
            state: "implementation".to_string(),
            force: false,
            approve_terminal_cascade: false,
            if_match: None,
            actor_kind: None,
            agent_name: None,
            agent_model: None,
            agent_version: None,
        }),
    }
}

#[test]
fn drain_pending_requests_processes_all_items_serially() {
    let root = unique_root();
    let paths = QueuePaths::for_repo(&root);
    paths.create_dirs().expect("queue dirs should exist");

    let first = state_request(&root, &paths, "first", "K-1");
    enqueue_request(&paths, &first).expect("first request should enqueue");
    thread::sleep(Duration::from_millis(2));

    let second = state_request(&root, &paths, "second", "K-2");
    enqueue_request(&paths, &second).expect("second request should enqueue");

    let mut seen = Vec::new();
    let processed = drain_pending_requests(&paths, &mut |request| {
        seen.push(request.request_id.clone());
        QueuedWriteResponse::success(request.request_id.clone())
    })
    .expect("drain should succeed");
    assert_eq!(processed, 2);
    assert_eq!(seen, vec!["first".to_string(), "second".to_string()]);

    let first_response = read_response_file(&paths.responses_dir.join("first.json"))
        .expect("first response should read")
        .expect("first response should exist");
    assert_eq!(first_response.output, "first");

    let second_response = read_response_file(&paths.responses_dir.join("second.json"))
        .expect("second response should read")
        .expect("second response should exist");
    assert_eq!(second_response.output, "second");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn enqueue_and_wait_round_trips_response() {
    let root = unique_root();
    let response = enqueue_and_wait(
        &root,
        ".knots/cache/state.sqlite",
        WriteOperation::Next(NextOperation {
            id: "K-123".to_string(),
            expected_state: Some("planning".to_string()),
            json: false,
            approve_terminal_cascade: false,
            actor_kind: None,
            agent_name: None,
            agent_model: None,
            agent_version: None,
            lease_id: None,
        }),
        |request| QueuedWriteResponse::success(request.request_id.clone()),
    )
    .expect("enqueue and wait should succeed");

    assert!(response.success);
    assert!(!response.output.is_empty());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn drain_pending_requests_returns_zero_when_worker_is_busy() {
    let root = unique_root();
    let paths = QueuePaths::for_repo(&root);
    paths.create_dirs().expect("queue dirs should exist");

    let _held = FileLock::try_acquire(&paths.worker_lock_path)
        .expect("worker lock acquisition should not fail")
        .expect("worker lock should be available");

    let processed = drain_pending_requests(&paths, &mut |_request| {
        QueuedWriteResponse::success("unexpected".to_string())
    })
    .expect("drain should return success when worker is busy");
    assert_eq!(processed, 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn drain_pending_requests_removes_invalid_request_files() {
    let root = unique_root();
    let paths = QueuePaths::for_repo(&root);
    paths.create_dirs().expect("queue dirs should exist");

    let invalid = paths.requests_dir.join("invalid.json");
    fs::write(&invalid, "{not valid json").expect("invalid request file should be writable");

    let err = drain_pending_requests(&paths, &mut |_request| {
        QueuedWriteResponse::success("unused".to_string())
    })
    .expect_err("invalid request file should fail");
    assert!(matches!(err, QueueError::Serde(_)));
    assert!(
        !invalid.exists(),
        "invalid request file should be removed after parse failure"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn enqueue_and_wait_spins_until_worker_lock_is_released() {
    let root = unique_root();
    let paths = QueuePaths::for_repo(&root);
    paths.create_dirs().expect("queue dirs should exist");

    let held = FileLock::try_acquire(&paths.worker_lock_path)
        .expect("worker lock acquisition should not fail")
        .expect("worker lock should be available");
    let releaser = thread::spawn(move || {
        thread::sleep(Duration::from_millis(60));
        drop(held);
    });

    let response = enqueue_and_wait(
        &root,
        ".knots/cache/state.sqlite",
        WriteOperation::Claim(ClaimOperation {
            id: "K-1".to_string(),
            json: false,
            verbose: false,
            agent_name: None,
            agent_model: None,
            agent_version: None,
            lease_id: None,
            timeout_seconds: None,
        }),
        |request| QueuedWriteResponse::success(request.request_id.clone()),
    )
    .expect("enqueue and wait should succeed once lock is released");
    assert!(response.success);
    assert!(!response.output.is_empty());
    releaser.join().expect("releaser thread should complete");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn queue_error_display_and_from_cover_all_variants() {
    let io_error = QueueError::from(std::io::Error::other("disk full"));
    assert!(format!("{io_error}").contains("queue I/O error"));

    let serde_error =
        QueueError::from(serde_json::from_str::<serde_json::Value>("{").expect_err("bad json"));
    assert!(format!("{serde_error}").contains("queue serialization error"));

    let lock_error = QueueError::from(LockError::Busy(PathBuf::from("busy.lock")));
    assert!(format!("{lock_error}").contains("queue lock error"));

    let timeout_error = QueueError::Timeout(Duration::from_millis(1));
    assert!(format!("{timeout_error}").contains("timed out waiting for queued write response"));
}

#[test]
fn list_request_files_and_read_response_file_handle_missing_paths() {
    let root = unique_root();

    let missing_requests = root.join("missing-requests");
    let files = list_request_files(&missing_requests).expect("missing dir should be treated empty");
    assert!(files.is_empty());

    let missing_response = root.join("missing-response.json");
    let response = read_response_file(&missing_response).expect("missing response should be ok");
    assert!(response.is_none());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn lease_create_operation_serializes_round_trip() {
    let op = WriteOperation::LeaseCreate(LeaseCreateOperation {
        nickname: "test-session".to_string(),
        lease_type: "agent".to_string(),
        agent_type: Some("cli".to_string()),
        provider: Some("Anthropic".to_string()),
        agent_name: Some("claude".to_string()),
        model: Some("opus".to_string()),
        model_version: Some("4.6".to_string()),
        json: false,
        timeout_seconds: None,
    });
    let json = serde_json::to_string(&op).expect("should serialize");
    let parsed: WriteOperation = serde_json::from_str(&json).expect("should deserialize");
    assert_eq!(parsed, op);
}

#[test]
fn lease_terminate_operation_serializes_round_trip() {
    let op = WriteOperation::LeaseTerminate(LeaseTerminateOperation {
        id: "knot-abc123".to_string(),
    });
    let json = serde_json::to_string(&op).expect("should serialize");
    let parsed: WriteOperation = serde_json::from_str(&json).expect("should deserialize");
    assert_eq!(parsed, op);
}

#[test]
fn lease_create_operation_with_no_optional_fields() {
    let op = WriteOperation::LeaseCreate(LeaseCreateOperation {
        nickname: "manual-session".to_string(),
        lease_type: "manual".to_string(),
        agent_type: None,
        provider: None,
        agent_name: None,
        model: None,
        model_version: None,
        json: false,
        timeout_seconds: None,
    });
    let json = serde_json::to_string(&op).expect("should serialize");
    let parsed: WriteOperation = serde_json::from_str(&json).expect("should deserialize");
    assert_eq!(parsed, op);
}

#[test]
fn lease_extend_operation_serializes_round_trip() {
    let op = WriteOperation::LeaseExtend(LeaseExtendOperation {
        lease_id: "knot-lease-1".to_string(),
        timeout_seconds: Some(1200),
        json: true,
    });
    let json = serde_json::to_string(&op).expect("should serialize");
    let parsed: WriteOperation = serde_json::from_str(&json).expect("should deserialize");
    assert_eq!(parsed, op);
}
