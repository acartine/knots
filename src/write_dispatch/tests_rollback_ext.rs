use super::helpers::{format_next_output, format_rollback_output, normalize_expected_state};
use super::*;
use crate::app::{App, StateActorMetadata};
use crate::domain::knot_type::KnotType;
use crate::write_queue::{RollbackOperation, StepAnnotateOperation, WriteOperation};

use std::path::{Path, PathBuf};
use std::process::Command;
use uuid::Uuid;

fn unique_workspace(prefix: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("temp workspace should be creatable");
    root
}

fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git").arg("-C").arg(root).args(args).output();
    let output = output.expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn setup_repo(root: &Path) {
    run_git(root, &["init"]);
    run_git(root, &["config", "user.email", "knots@example.com"]);
    run_git(root, &["config", "user.name", "Knots Test"]);
    std::fs::write(root.join("README.md"), "# knots\n").expect("readme should be writable");
    run_git(root, &["add", "README.md"]);
    run_git(root, &["commit", "-m", "init"]);
    run_git(root, &["branch", "-M", "main"]);
}

#[test]
fn execute_operation_rollback_covers_rejection_path() {
    let root = unique_workspace("knots-wd-rb-reject");
    setup_repo(&root);
    let db_path = root.join(".knots/cache/state.sqlite");
    let app = App::open(
        db_path.to_str().expect("db path should be utf8"),
        root.clone(),
    )
    .expect("app should open");
    let created = app
        .create_knot("Rollback", None, Some("ready_for_implementation"), None)
        .expect("knot should be created");
    let err = execute_operation(
        &app,
        &WriteOperation::Rollback(RollbackOperation {
            id: created.id.clone(),
            dry_run: false,
            actor_kind: None,
            agent_name: None,
            agent_model: None,
            agent_version: None,
        }),
    )
    .expect_err("queue-state rollback should fail");
    match err {
        AppError::InvalidArgument(msg) => {
            assert!(msg.contains("queue state"))
        }
        other => panic!("unexpected rollback rejection: {other}"),
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn execute_operation_rollback_covers_dry_run_and_real_paths() {
    let root = unique_workspace("knots-wd-rb-paths");
    setup_repo(&root);
    let db_path = root.join(".knots/cache/state.sqlite");
    let app = App::open(
        db_path.to_str().expect("db path should be utf8"),
        root.clone(),
    )
    .expect("app should open");
    let created = app
        .create_knot("Rollback", None, Some("ready_for_implementation"), None)
        .expect("knot should be created");
    let implementation = app
        .set_state_with_actor(
            &created.id,
            "implementation",
            false,
            created.profile_etag.as_deref(),
            StateActorMetadata {
                actor_kind: Some("agent".to_string()),
                agent_name: Some("claimer".to_string()),
                agent_model: Some("model".to_string()),
                agent_version: Some("1.0".to_string()),
            },
        )
        .expect("implementation claim should succeed");
    let dry_run = execute_operation(
        &app,
        &WriteOperation::Rollback(RollbackOperation {
            id: implementation.id.clone(),
            dry_run: true,
            actor_kind: None,
            agent_name: None,
            agent_model: None,
            agent_version: None,
        }),
    )
    .expect("dry-run rollback should succeed");
    assert!(dry_run.contains("would roll back"));
    assert!(dry_run.contains("ready_for_implementation"));

    let after = app
        .show_knot(&implementation.id)
        .expect("knot should load")
        .expect("knot should exist");
    assert_eq!(after.state, "implementation");

    advance_and_rollback(&app, &implementation);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn rollback_releases_bound_lease() {
    let root = unique_workspace("knots-wd-rb-lease");
    setup_repo(&root);
    let db_path = root.join(".knots/cache/state.sqlite");
    let app = App::open(
        db_path.to_str().expect("db path should be utf8"),
        root.clone(),
    )
    .expect("app should open");
    let created = app
        .create_knot(
            "Rollback with lease",
            None,
            Some("work_item"),
            Some("default"),
        )
        .expect("knot should be created");
    let claimed = crate::poll_claim::claim_knot(
        &app,
        &created.id,
        StateActorMetadata {
            actor_kind: Some("agent".to_string()),
            agent_name: Some("test-agent".to_string()),
            agent_model: Some("test-model".to_string()),
            agent_version: Some("1.0".to_string()),
        },
        None,
        600,
    )
    .expect("claim should succeed");
    let lease_id = claimed
        .knot
        .lease_id
        .clone()
        .expect("lease should be bound after claim");

    let output = execute_operation(
        &app,
        &WriteOperation::Rollback(RollbackOperation {
            id: created.id.clone(),
            dry_run: false,
            actor_kind: Some("agent".to_string()),
            agent_name: None,
            agent_model: None,
            agent_version: None,
        }),
    )
    .expect("rollback should succeed");
    assert!(output.contains("rolled back"));

    let after = app
        .show_knot(&created.id)
        .expect("knot should load")
        .expect("knot should exist");
    assert!(
        after.lease_id.is_none(),
        "rollback should unbind lease from knot"
    );

    let lease = app
        .show_knot(&lease_id)
        .expect("lease should load")
        .expect("lease should exist");
    assert_eq!(
        lease.state, "lease_terminated",
        "rollback should terminate the bound lease"
    );

    let _ = std::fs::remove_dir_all(root);
}

fn advance_and_rollback(app: &App, implementation: &crate::app::KnotView) {
    app.set_state_with_actor(
        &implementation.id,
        "ready_for_implementation_review",
        false,
        implementation.profile_etag.as_deref(),
        StateActorMetadata::default(),
    )
    .expect("queue review transition should succeed");
    let in_review = app
        .set_state_with_actor(
            &implementation.id,
            "implementation_review",
            false,
            None,
            StateActorMetadata {
                actor_kind: Some("agent".to_string()),
                agent_name: Some("reviewer".to_string()),
                agent_model: Some("model".to_string()),
                agent_version: Some("2.0".to_string()),
            },
        )
        .expect("review claim should succeed");
    let output = execute_operation(
        app,
        &WriteOperation::Rollback(RollbackOperation {
            id: in_review.id.clone(),
            dry_run: false,
            actor_kind: Some("agent".to_string()),
            agent_name: Some("rollbacker".to_string()),
            agent_model: Some("model".to_string()),
            agent_version: Some("3.0".to_string()),
        }),
    )
    .expect("rollback should succeed");
    assert!(output.contains("rolled back"));
    assert!(output.contains("ready_for_implementation"));

    let after = app
        .show_knot(&in_review.id)
        .expect("knot should load")
        .expect("knot should exist");
    assert_eq!(after.state, "ready_for_implementation");
}

#[test]
fn normalize_and_format_helpers() {
    assert_eq!(
        normalize_expected_state("implemented"),
        "ready_for_implementation_review"
    );
    assert_eq!(normalize_expected_state("State-Name"), "state_name");

    let knot = crate::app::KnotView {
        id: "knots-1".to_string(),
        alias: Some("root.1".to_string()),
        title: "Example".to_string(),
        state: "planning".to_string(),
        updated_at: "2026-03-10T00:00:00Z".to_string(),
        body: None,
        description: None,
        acceptance: None,
        priority: None,
        knot_type: KnotType::Work,
        tags: vec![],
        notes: vec![],
        handoff_capsules: vec![],
        invariants: vec![],
        step_history: vec![],
        gate: None,
        lease: None,
        lease_id: None,
        lease_expiry_ts: 0,
        lease_agent: None,
        workflow_id: "work_sdlc".to_string(),
        profile_id: "autopilot".to_string(),
        profile_etag: None,
        deferred_from_state: None,
        blocked_from_state: None,
        created_at: None,
        step_metadata: None,
        next_step_metadata: None,
        edges: vec![],
        child_summaries: vec![],
    };

    let text = format_next_output(&knot, "idea", Some("agent"), false);
    assert!(text.contains("root.1"));
    assert!(text.contains("owner: agent"));

    let json = format_next_output(&knot, "idea", None, true);
    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("json next output should parse");
    assert_eq!(parsed["previous_state"], "idea");
    assert_eq!(parsed["state"], "planning");

    let rb = format_rollback_output(
        &knot,
        "ready_for_implementation",
        Some("agent"),
        "implementation is an action state",
        true,
    );
    assert!(rb.contains("would roll back"));
    assert!(rb.contains("owner: agent"));
}

#[test]
fn execute_operation_step_annotate_text_and_json() {
    let root = unique_workspace("knots-wd-step-annotate");
    setup_repo(&root);
    let db_path = root.join(".knots/cache/state.sqlite");
    let app = App::open(
        db_path.to_str().expect("db path should be utf8"),
        root.clone(),
    )
    .expect("app should open");
    let created = app
        .create_knot("Step annotate", None, Some("ready_for_planning"), None)
        .expect("knot should be created");
    let claimed = app
        .set_state_with_actor(
            &created.id,
            "planning",
            false,
            created.profile_etag.as_deref(),
            StateActorMetadata {
                actor_kind: Some("agent".to_string()),
                agent_name: Some("claimer".to_string()),
                agent_model: Some("model".to_string()),
                agent_version: Some("1.0".to_string()),
            },
        )
        .expect("claim should start a step");

    let text = execute_operation(
        &app,
        &WriteOperation::StepAnnotate(StepAnnotateOperation {
            id: claimed.id.clone(),
            actor_kind: Some("agent".to_string()),
            agent_name: Some("annotator".to_string()),
            agent_model: Some("model".to_string()),
            agent_version: Some("2.0".to_string()),
            json: false,
        }),
    )
    .expect("text step annotate should succeed");
    assert!(text.contains("step annotated"));

    let json = execute_operation(
        &app,
        &WriteOperation::StepAnnotate(StepAnnotateOperation {
            id: claimed.id.clone(),
            actor_kind: Some("agent".to_string()),
            agent_name: Some("annotator-json".to_string()),
            agent_model: Some("model".to_string()),
            agent_version: Some("3.0".to_string()),
            json: true,
        }),
    )
    .expect("json step annotate should succeed");
    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("json step annotate output should parse");
    assert_eq!(parsed["id"], claimed.id);
    assert!(parsed["step_history"].as_array().is_some());
}
