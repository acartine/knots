use super::*;

use std::path::Path;
use std::process::Command;

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
Ship {{ output }} output.
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

fn unique_workspace() -> std::path::PathBuf {
    let root =
        std::env::temp_dir().join(format!("knots-run-command-test-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    root
}

fn setup_git_repo(root: &Path) {
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("git should run");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run(&["init"]);
    run(&["config", "user.email", "knots@example.com"]);
    run(&["config", "user.name", "Knots Test"]);
    std::fs::write(root.join("README.md"), "# knots\n").expect("readme should be writable");
    run(&["add", "README.md"]);
    run(&["commit", "-m", "init"]);
    run(&["branch", "-M", "main"]);
}

fn install_custom_workflow(root: &Path) {
    let source = root.join("custom-flow.toml");
    std::fs::write(&source, CUSTOM_BUNDLE).expect("bundle should write");
    crate::installed_workflows::install_bundle(root, &source).expect("bundle should install");
    crate::installed_workflows::set_current_workflow_selection(
        root,
        "custom_flow",
        Some(3),
        Some("autopilot"),
    )
    .expect("workflow selection should succeed");
}

#[test]
fn resolve_skill_by_name_uses_current_workflow_prompt() {
    let root = unique_workspace();
    install_custom_workflow(&root);
    let db_path = root.join(".knots/cache/state.sqlite");
    let app = app::App::open(db_path.to_str().expect("utf8"), root.clone()).expect("app");

    let skill = resolve_skill_by_name(&app, "work").expect("custom prompt should resolve");
    assert!(skill.contains("Ship {{ output }} output."));
    assert!(skill.contains("## Acceptance Criteria"));
    assert!(skill.contains("Built output"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn resolve_skill_by_name_builtin_returns_loom_body_for_implementation() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app = app::App::open(db_path.to_str().expect("utf8"), root.clone()).expect("app");

    let skill = resolve_skill_by_name(&app, "implementation")
        .expect("builtin implementation should resolve");
    assert!(
        skill.contains("# Implementation"),
        "builtin skill should contain Loom heading: {skill}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn resolve_skill_by_name_builtin_covers_all_loom_action_states() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app = app::App::open(db_path.to_str().expect("utf8"), root.clone()).expect("app");

    let states_and_headings = [
        ("planning", "# Planning"),
        ("plan_review", "# Plan Review"),
        ("implementation", "# Implementation"),
        ("implementation_review", "# Implementation Review"),
        ("shipment", "# Shipment"),
        ("shipment_review", "# Shipment Review"),
        ("evaluating", "# Evaluating"),
    ];
    for (state, heading) in states_and_headings {
        let skill = resolve_skill_by_name(&app, state).unwrap_or_else(|e| panic!("{state}: {e}"));
        assert!(
            skill.contains(heading),
            "{state}: skill should contain Loom heading '{heading}'"
        );
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn resolve_skill_for_knot_returns_loom_body_for_builtin_profile() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app = app::App::open(db_path.to_str().expect("utf8"), root.clone()).expect("app");

    let knot = app
        .create_knot("Skill knot test", None, Some("work_item"), None)
        .expect("create");
    let skill =
        resolve_skill_for_knot(&app, &knot, &knot.id).expect("should resolve skill for knot");
    assert!(
        skill.contains("# Implementation"),
        "skill for knot should contain Loom heading"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn resolve_skill_for_knot_custom_workflow_returns_loom_body() {
    let root = unique_workspace();
    install_custom_workflow(&root);
    let db_path = root.join(".knots/cache/state.sqlite");
    let app = app::App::open(db_path.to_str().expect("utf8"), root.clone()).expect("app");

    let knot = app
        .create_knot("Custom skill knot", None, None, None)
        .expect("create");
    let skill = resolve_skill_for_knot(&app, &knot, &knot.id)
        .expect("should resolve skill for custom knot");
    assert!(
        skill.contains("Ship"),
        "custom knot skill should resolve Loom body content, got: {skill}"
    );
    assert!(
        skill.contains("Built output"),
        "custom knot skill should include acceptance criteria"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn resolve_skill_by_name_rejects_legacy_fallbacks_for_custom_workflows() {
    let root = unique_workspace();
    install_custom_workflow(&root);
    let db_path = root.join(".knots/cache/state.sqlite");
    let app = app::App::open(db_path.to_str().expect("utf8"), root.clone()).expect("app");

    let err = resolve_skill_by_name(&app, "implementation")
        .expect_err("missing custom state should not fall back");
    assert!(format!("{err}").contains("not a knot id or skill state name"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn show_json_value_hides_lease_id_and_keeps_lease_agent_metadata() {
    let knot = app::KnotView {
        id: "K-lease-bound".to_string(),
        alias: None,
        title: "Lease-bound work".to_string(),
        state: "implementation".to_string(),
        updated_at: "2026-04-03T00:00:00Z".to_string(),
        body: None,
        description: None,
        acceptance: None,
        priority: None,
        knot_type: crate::domain::knot_type::KnotType::Work,
        tags: Vec::new(),
        notes: Vec::new(),
        handoff_capsules: Vec::new(),
        invariants: Vec::new(),
        step_history: Vec::new(),
        gate: None,
        lease: None,
        lease_id: Some("knots-lease-secret".to_string()),
        lease_expiry_ts: 0,
        lease_agent: Some(crate::domain::lease::AgentInfo {
            agent_type: "cli".to_string(),
            provider: "Anthropic".to_string(),
            agent_name: "claude".to_string(),
            model: "opus".to_string(),
            model_version: "4.6".to_string(),
        }),
        workflow_id: "compatibility".to_string(),
        profile_id: "autopilot".to_string(),
        profile_etag: None,
        deferred_from_state: None,
        blocked_from_state: None,
        created_at: None,
        step_metadata: None,
        next_step_metadata: None,
        edges: Vec::new(),
        child_summaries: Vec::new(),
    };

    let value = show_json_value(&knot);
    assert!(
        value.get("lease_id").is_none(),
        "generic show JSON must not expose lease_id"
    );
    assert_eq!(value["lease_agent"]["provider"].as_str(), Some("Anthropic"));
    assert_eq!(value["lease_agent"]["agent_name"].as_str(), Some("claude"));
    assert_eq!(value["lease_agent"]["model"].as_str(), Some("opus"));
    assert_eq!(value["lease_agent"]["model_version"].as_str(), Some("4.6"));
}

#[test]
fn run_show_rejects_lease_knots_but_lease_show_still_allows_them() {
    let root = unique_workspace();
    setup_git_repo(&root);
    let db_path = root.join(".knots/cache/state.sqlite");
    let app = app::App::open(db_path.to_str().expect("utf8"), root.clone()).expect("app");

    let lease = crate::lease::create_lease(
        &app,
        "showable-lease",
        crate::domain::lease::LeaseType::Agent,
        Some(crate::domain::lease::AgentInfo {
            agent_type: "cli".to_string(),
            provider: "Anthropic".to_string(),
            agent_name: "claude".to_string(),
            model: "opus".to_string(),
            model_version: "4.6".to_string(),
        }),
        600,
    )
    .expect("lease should be created");

    let text_err = run_show(
        &app,
        crate::cli::ShowArgs {
            id: lease.id.clone(),
            json: false,
            verbose: false,
        },
    )
    .expect_err("generic text show should reject lease knots");
    assert!(text_err.to_string().contains("kno lease show"));

    let json_err = run_show(
        &app,
        crate::cli::ShowArgs {
            id: lease.id.clone(),
            json: true,
            verbose: false,
        },
    )
    .expect_err("generic json show should reject lease knots");
    assert!(json_err.to_string().contains("kno lease show"));

    run_lease_read(
        &app,
        crate::cli::LeaseArgs {
            command: crate::cli::LeaseSubcommands::Show(crate::cli::LeaseShowArgs {
                id: lease.id.clone(),
                json: false,
            }),
        },
    )
    .expect("lease show text should remain available");

    run_lease_read(
        &app,
        crate::cli::LeaseArgs {
            command: crate::cli::LeaseSubcommands::Show(crate::cli::LeaseShowArgs {
                id: lease.id.clone(),
                json: true,
            }),
        },
    )
    .expect("lease show json should remain available");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn run_ls_stream_succeeds_with_knots() {
    let root = unique_workspace();
    setup_git_repo(&root);
    let db_path = root.join(".knots/cache/state.sqlite");
    let app = app::App::open(db_path.to_str().expect("utf8"), root.clone()).expect("app");

    app.create_knot("Stream test knot", None, Some("work_item"), None)
        .expect("create");

    // stream=true exercises stream_ndjson_knots -> write_ndjson path
    run_ls(
        &app,
        crate::cli::ListArgs {
            all: false,
            json: false,
            state: None,
            knot_type: None,
            profile_id: None,
            tags: Vec::new(),
            query: None,
            stream: true,
            limit: None,
            offset: None,
        },
    )
    .expect("stream ls should succeed");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn run_ls_stream_with_limit_caps_output() {
    let root = unique_workspace();
    setup_git_repo(&root);
    let db_path = root.join(".knots/cache/state.sqlite");
    let app = app::App::open(db_path.to_str().expect("utf8"), root.clone()).expect("app");

    app.create_knot("Limit A", None, Some("work_item"), None)
        .expect("create");
    app.create_knot("Limit B", None, Some("work_item"), None)
        .expect("create");

    // stream=true with limit=1 exercises the truncate + stream path
    run_ls(
        &app,
        crate::cli::ListArgs {
            all: false,
            json: false,
            state: None,
            knot_type: None,
            profile_id: None,
            tags: Vec::new(),
            query: None,
            stream: true,
            limit: Some(1),
            offset: None,
        },
    )
    .expect("stream ls with limit should succeed");

    let _ = std::fs::remove_dir_all(root);
}
