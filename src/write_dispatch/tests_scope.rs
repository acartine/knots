use std::path::Path;
use std::process::Command;

use super::execute_operation;
use crate::app::App;
use crate::cli_scope::ScopeArgs;
use crate::domain::scope::ScopePatch;
use crate::write_queue::{NewOperation, UpdateOperation, WriteOperation};

fn unique_workspace() -> knots_test_support::TestWorkspace {
    knots_test_support::workspace("knots-scope-test")
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

fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn new_and_update_scope_round_trip() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");
    let app = App::open(db.to_str().expect("utf8 path"), root).expect("app should open");

    execute_operation(
        &app,
        &WriteOperation::New(NewOperation {
            title: "scope smoke".to_string(),
            description: None,
            acceptance: None,
            verification_steps: vec![],
            state: None,
            profile: None,
            workflow: None,
            fast: false,
            exploration: false,
            knot_type: None,
            objective: None,
            gate_owner_kind: None,
            gate_failure_modes: vec![],
            tags: vec![],
            scope: ScopeArgs {
                scope_volume: Some("5".to_string()),
                scope_scale: Some("fib_v1".to_string()),
                scope_reliability: Some("62".to_string()),
                ..ScopeArgs::default()
            },
            lease_id: None,
            json: false,
        }),
    )
    .expect("new should succeed");
    let created = app
        .list_knots()
        .expect("list should succeed")
        .into_iter()
        .find(|knot| knot.title == "scope smoke")
        .expect("created knot should exist");
    assert_eq!(created.scope.as_ref().and_then(|s| s.volume), Some(5));
    assert_eq!(
        created.scope.as_ref().and_then(|s| s.scale.as_deref()),
        Some("fib_v1")
    );

    execute_operation(
        &app,
        &WriteOperation::Update(UpdateOperation {
            id: created.id.clone(),
            scope: ScopeArgs {
                scope_volume: Some("8".to_string()),
                scope_scale: Some("fib_v1".to_string()),
                scope_volume_score_confidence: Some("0.72".to_string()),
                scope_volume_stddev: Some("1.25".to_string()),
                scope_volume_result_id: Some("vol-1".to_string()),
                scope_reliability: Some("44".to_string()),
                scope_reliability_score_confidence: Some("0.91".to_string()),
                scope_reliability_stddev: Some("2.5".to_string()),
                scope_reliability_band: Some("medium".to_string()),
                scope_reliability_result_id: Some("rel-1".to_string()),
            },
            ..empty_update()
        }),
    )
    .expect("update should succeed");
    let updated = app
        .show_knot(&created.id)
        .expect("show should succeed")
        .expect("knot should exist");
    let scope = updated.scope.expect("scope should be present");
    assert_eq!(scope.volume, Some(8));
    assert_eq!(scope.scale.as_deref(), Some("fib_v1"));
    assert_eq!(scope.reliability, Some(44));
    assert_eq!(scope.volume_score_confidence.unwrap().get(), 0.72);
    assert_eq!(scope.volume_stddev.unwrap().get(), 1.25);
    assert_eq!(scope.volume_result_id.as_deref(), Some("vol-1"));
    assert_eq!(scope.reliability_score_confidence.unwrap().get(), 0.91);
    assert_eq!(scope.reliability_stddev.unwrap().get(), 2.5);
    assert_eq!(scope.reliability_band.as_deref(), Some("medium"));
    assert_eq!(scope.reliability_result_id.as_deref(), Some("rel-1"));
}

#[test]
fn app_scope_update_rejects_empty_patch_and_noops_same_value() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");
    let app = App::open(db.to_str().expect("utf8 path"), root).expect("app should open");

    let created = app
        .create_knot("scope no-op", None, None, None)
        .expect("knot should be created");
    let err = app
        .update_knot_scope(&created.id, ScopePatch::default(), None)
        .expect_err("empty patch should be rejected");
    assert!(err.to_string().contains("at least one field"));

    let updated = app
        .update_knot_scope(
            &created.id,
            ScopePatch {
                volume: Some(5),
                ..ScopePatch::default()
            },
            None,
        )
        .expect("scope should update");
    let etag = updated.profile_etag.clone();
    let noop = app
        .update_knot_scope(
            &created.id,
            ScopePatch {
                volume: Some(5),
                ..ScopePatch::default()
            },
            etag.as_deref(),
        )
        .expect("same value should be a no-op");
    assert_eq!(noop.scope.as_ref().and_then(|scope| scope.volume), Some(5));
    assert_eq!(noop.profile_etag, etag);
}

fn empty_update() -> UpdateOperation {
    UpdateOperation {
        id: String::new(),
        title: None,
        description: None,
        acceptance: None,
        priority: None,
        status: None,
        knot_type: None,
        add_tags: vec![],
        remove_tags: vec![],
        add_invariants: vec![],
        remove_invariants: vec![],
        clear_invariants: false,
        add_verification_steps: vec![],
        remove_verification_steps: vec![],
        clear_verification_steps: false,
        gate_owner_kind: None,
        gate_failure_modes: vec![],
        clear_gate_failure_modes: false,
        scope: ScopeArgs::default(),
        execution_plan_file: None,
        objective: None,
        add_note: None,
        note_username: None,
        note_datetime: None,
        note_agentname: None,
        note_model: None,
        note_version: None,
        add_handoff_capsule: None,
        handoff_username: None,
        handoff_datetime: None,
        handoff_agentname: None,
        handoff_model: None,
        handoff_version: None,
        if_match: None,
        actor_kind: None,
        agent_name: None,
        agent_model: None,
        agent_version: None,
        force: false,
        approve_terminal_cascade: false,
        lease_id: None,
        json: false,
    }
}
