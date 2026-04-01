use super::*;

use std::path::Path;

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
