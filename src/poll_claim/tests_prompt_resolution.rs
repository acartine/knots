use std::path::PathBuf;

use super::*;

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-prompt-res-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    root
}

fn open_app(root: &std::path::Path) -> App {
    let db_path = root.join(".knots/cache/state.sqlite");
    App::open(db_path.to_str().expect("utf8"), root.to_path_buf()).expect("app should open")
}

/// Custom workflow bundle with a simple produce/gate two-step flow.
/// The work prompt uses the raw Loom body "Perform the work." (no
/// template variables) so claim/poll assertions match exactly.
const CUSTOM_BUNDLE: &str = r#"
[workflow]
name = "custom_flow"
version = 3
default_profile = "autopilot"

[states.ready_for_work]
display_name = "Ready for Work"
kind = "queue"

[states.work]
display_name = "Work"
kind = "action"
action_type = "produce"
executor = "agent"
prompt = "work"
output = "branch"

[states.review]
display_name = "Review"
kind = "action"
action_type = "gate"
executor = "human"
prompt = "review"
output = "note"

[states.ready_for_review]
display_name = "Ready for Review"
kind = "queue"

[states.done]
display_name = "Done"
kind = "terminal"

[states.blocked]
display_name = "Blocked"
kind = "escape"

[states.deferred]
display_name = "Deferred"
kind = "escape"

[states.abandoned]
display_name = "Abandoned"
kind = "terminal"

[steps.impl]
queue = "ready_for_work"
action = "work"

[steps.rev]
queue = "ready_for_review"
action = "review"

[phases.main]
produce = "impl"
gate = "rev"

[profiles.autopilot]
description = "Custom profile"
phases = ["main"]

[prompts.work]
accept = ["Built output"]
body = """
Perform the work.
"""

[prompts.work.success]
complete = "ready_for_review"

[prompts.work.failure]
blocked = "blocked"

[prompts.review]
accept = ["Reviewed output"]
body = """
Review it.
"""

[prompts.review.success]
approved = "done"

[prompts.review.failure]
changes = "ready_for_work"
"#;

fn install_custom_workflow(root: &std::path::Path) {
    let source = root.join("custom-flow.toml");
    std::fs::write(&source, CUSTOM_BUNDLE).expect("bundle write");
    crate::installed_workflows::install_bundle(root, &source).expect("bundle should install");
    crate::installed_workflows::set_current_workflow_selection(
        root,
        "custom_flow",
        Some(3),
        Some("autopilot"),
    )
    .expect("workflow should select");
}

// ── prompt_body_for_state: builtin profiles ──────────────

#[test]
fn builtin_profile_resolves_all_action_state_prompts() {
    let root = unique_workspace();
    let registry =
        crate::profile::ProfileRegistry::load_for_repo(&root).expect("registry should load");
    for state in [
        "planning",
        "plan_review",
        "implementation",
        "implementation_review",
        "shipment",
        "shipment_review",
    ] {
        let body = prompt_body_for_state(&registry, "autopilot", state);
        assert!(
            body.is_ok(),
            "autopilot should resolve prompt for {state}: {:?}",
            body.err()
        );
        assert!(
            !body.unwrap().is_empty(),
            "prompt body for {state} should not be empty"
        );
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn builtin_profile_prompt_comes_from_loom_body_not_raw_skill() {
    let root = unique_workspace();
    let registry =
        crate::profile::ProfileRegistry::load_for_repo(&root).expect("registry should load");

    // The builtin compatibility workflow renders prompts through
    // Loom template resolution (output-specific sections resolved).
    // For autopilot profiles the output=remote_main lines should be
    // kept and the output=pr lines removed.
    let body = prompt_body_for_state(&registry, "autopilot", "implementation")
        .expect("prompt should resolve");

    // Rendered prompt should NOT contain raw template markers.
    assert!(
        !body.contains("`{{ output }}` = `remote_main`"),
        "output-specific markers should be resolved"
    );
    assert!(
        !body.contains("`{{ output }}` = `pr`"),
        "output-specific markers should be resolved"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn builtin_pr_profile_resolves_pr_specific_prompt_body() {
    let root = unique_workspace();
    let registry =
        crate::profile::ProfileRegistry::load_for_repo(&root).expect("registry should load");

    let body = prompt_body_for_state(&registry, "autopilot_with_pr", "implementation")
        .expect("prompt should resolve");

    assert!(
        body.contains("pull request"),
        "PR profile should mention pull request"
    );
    let _ = std::fs::remove_dir_all(root);
}

// ── prompt_body_for_state: acceptance criteria ───────────

#[test]
fn prompt_body_appends_acceptance_criteria_from_profile() {
    let root = unique_workspace();
    install_custom_workflow(&root);
    let registry =
        crate::profile::ProfileRegistry::load_for_repo(&root).expect("registry should load");
    let body = prompt_body_for_state(&registry, "custom_flow/autopilot", "work")
        .expect("prompt should resolve");
    assert!(
        body.contains("## Acceptance Criteria"),
        "should include acceptance section: {body}"
    );
    assert!(
        body.contains("Built output"),
        "should contain acceptance item: {body}"
    );
    let _ = std::fs::remove_dir_all(root);
}

// ── prompt_body_for_state: custom workflow ───────────────

#[test]
fn custom_workflow_profile_resolves_prompt_from_loom_body() {
    let root = unique_workspace();
    install_custom_workflow(&root);
    let registry =
        crate::profile::ProfileRegistry::load_for_repo(&root).expect("registry should load");
    let body = prompt_body_for_state(&registry, "custom_flow/autopilot", "work")
        .expect("prompt should resolve");
    assert!(
        body.contains("Perform the work."),
        "custom prompt should contain Loom body: {body}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn custom_workflow_review_prompt_resolves_from_loom_body() {
    let root = unique_workspace();
    install_custom_workflow(&root);
    let registry =
        crate::profile::ProfileRegistry::load_for_repo(&root).expect("registry should load");
    let body = prompt_body_for_state(&registry, "custom_flow/autopilot", "review")
        .expect("review prompt should resolve");
    assert!(
        body.contains("Review it."),
        "review prompt should contain Loom body: {body}"
    );
    assert!(
        body.contains("Reviewed output"),
        "review prompt should append acceptance: {body}"
    );
    let _ = std::fs::remove_dir_all(root);
}

// ── prompt_body_for_state: error paths ───────────────────

#[test]
fn prompt_body_for_unknown_state_returns_error() {
    let root = unique_workspace();
    let registry =
        crate::profile::ProfileRegistry::load_for_repo(&root).expect("registry should load");
    let result = prompt_body_for_state(&registry, "autopilot", "nonexistent_state");
    assert!(result.is_err(), "unknown state should return error");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("not an action state with a prompt"),
        "error should describe missing prompt: {err}"
    );
    let _ = std::fs::remove_dir_all(root);
}

// ── claim/poll with custom workflow prompts ──────────────

#[test]
fn claim_with_custom_workflow_returns_loom_prompt_body() {
    let root = unique_workspace();
    install_custom_workflow(&root);
    let app = open_app(&root);
    let knot = app
        .create_knot(
            "Custom prompt claim",
            None,
            None,
            Some("custom_flow/autopilot"),
        )
        .expect("create should succeed");

    let actor = StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: Some("test".to_string()),
        agent_model: None,
        agent_version: None,
    };
    let result = claim_knot(&app, &knot.id, actor, None).expect("claim should succeed");

    assert!(
        result.skill.contains("Perform the work."),
        "claimed prompt should contain Loom body"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn peek_with_custom_workflow_returns_loom_prompt_body() {
    let root = unique_workspace();
    install_custom_workflow(&root);
    let app = open_app(&root);
    let knot = app
        .create_knot(
            "Custom prompt peek",
            None,
            None,
            Some("custom_flow/autopilot"),
        )
        .expect("create should succeed");
    let result = peek_knot(&app, &knot.id).expect("peek should succeed");
    assert!(
        result.skill.contains("Perform the work."),
        "peeked prompt should contain Loom body"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn poll_queue_with_custom_workflow_returns_loom_prompt_body() {
    let root = unique_workspace();
    install_custom_workflow(&root);
    let app = open_app(&root);
    app.create_knot(
        "Custom prompt poll",
        None,
        None,
        Some("custom_flow/autopilot"),
    )
    .expect("create should succeed");

    let result = poll_queue(&app, None, Some("agent"))
        .expect("poll should succeed")
        .expect("poll should find a knot");

    assert!(
        result.skill.contains("Perform the work."),
        "polled prompt should contain Loom body"
    );
    let _ = std::fs::remove_dir_all(root);
}

// ── builtin and custom coexist ───────────────────────────

#[test]
fn builtin_knots_still_resolve_after_custom_workflow_installed() {
    let root = unique_workspace();
    install_custom_workflow(&root);
    let app = open_app(&root);

    // Use no-planning profile so initial state is
    // ready_for_implementation → implementation prompt
    let builtin_knot = app
        .create_knot("Builtin knot", None, None, Some("autopilot_no_planning"))
        .expect("builtin knot should create");

    let result = peek_knot(&app, &builtin_knot.id).expect("peek builtin should succeed");

    assert!(
        !result.skill.contains("Perform the work."),
        "builtin prompt should not contain custom flow text"
    );
    assert!(
        result.skill.contains("# Implementation"),
        "builtin prompt should contain implementation header"
    );
    let _ = std::fs::remove_dir_all(root);
}
