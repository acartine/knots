use std::path::{Path, PathBuf};

use crate::action_prompt;
use crate::installed_workflows::{self, InstalledWorkflowRegistry};
use crate::loom_compat_bundle;
use crate::poll_claim;
use crate::profile::ProfileRegistry;

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
# Custom Work

Ship {{ output }} output.
"""

[prompts.work.success]
complete = "ready_for_review"

[prompts.work.failure]
blocked = "blocked"

[prompts.review]
accept = ["Reviewed output"]
body = """
# Custom Review

Review it.
"""

[prompts.review.success]
approved = "done"

[prompts.review.failure]
changes = "ready_for_work"
"#;

fn unique_workspace(prefix: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&path).expect("workspace should be creatable");
    path
}

fn install_custom_workflow(root: &Path) {
    let source = root.join("custom-flow.toml");
    std::fs::write(&source, CUSTOM_BUNDLE).expect("bundle should write");
    installed_workflows::install_bundle(root, &source).expect("bundle should install");
    installed_workflows::set_current_workflow_selection(
        root,
        "custom_flow",
        Some(3),
        Some("autopilot"),
    )
    .expect("workflow selection should succeed");
}

// ── Builtin compat workflow resolves prompts from Loom bodies ──

#[test]
fn builtin_compat_profiles_resolve_planning_from_loom_body() {
    let root = unique_workspace("knots-compat-prompt-planning");
    let registry = ProfileRegistry::load_for_repo(&root).expect("registry");
    let loom_body = loom_compat_bundle::prompt_body_for_state("planning")
        .expect("loom bundle should have planning");

    for profile_id in ["autopilot", "semiauto", "autopilot_with_pr"] {
        let profile = registry.require(profile_id).expect(profile_id);
        let resolved = profile
            .prompt_for_action_state("planning")
            .expect("planning prompt should exist");
        assert!(
            resolved.contains("# Planning"),
            "{profile_id}: resolved prompt should contain Loom heading"
        );
        assert!(
            resolved.contains(loom_body.trim().lines().last().unwrap()),
            "{profile_id}: resolved prompt should contain Loom body content"
        );
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn builtin_compat_profiles_resolve_implementation_from_loom_body() {
    let root = unique_workspace("knots-compat-prompt-impl");
    let registry = ProfileRegistry::load_for_repo(&root).expect("registry");

    for profile_id in ["autopilot", "semiauto", "autopilot_no_planning"] {
        let profile = registry.require(profile_id).expect(profile_id);
        let resolved = profile
            .prompt_for_action_state("implementation")
            .expect("implementation prompt should exist");
        assert!(
            resolved.contains("# Implementation"),
            "{profile_id}: resolved should contain Loom heading"
        );
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn builtin_compat_all_action_states_have_loom_sourced_prompts() {
    let root = unique_workspace("knots-compat-all-states");
    let registry = ProfileRegistry::load_for_repo(&root).expect("registry");
    let profile = registry.require("autopilot").expect("autopilot");

    let loom_states = [
        ("planning", "# Planning"),
        ("plan_review", "# Plan Review"),
        ("implementation", "# Implementation"),
        ("implementation_review", "# Implementation Review"),
        ("shipment", "# Shipment"),
        ("shipment_review", "# Shipment Review"),
        ("evaluating", "# Evaluating"),
    ];

    for (state, expected_heading) in loom_states {
        let loom_body = loom_compat_bundle::prompt_body_for_state(state)
            .unwrap_or_else(|| panic!("loom bundle missing {state}"));
        let resolved = profile
            .prompt_for_action_state(state)
            .unwrap_or_else(|| panic!("profile missing prompt for {state}"));
        assert!(
            resolved.contains(expected_heading),
            "{state}: prompt should contain Loom heading '{expected_heading}'"
        );
        assert!(!resolved.is_empty(), "{state}: prompt should not be empty");
        assert!(
            loom_body.contains(expected_heading),
            "{state}: loom body should contain heading"
        );
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn builtin_render_for_profile_returns_loom_body_content() {
    let root = unique_workspace("knots-compat-render-profile");
    let registry = ProfileRegistry::load_for_repo(&root).expect("registry");
    let profile = registry.require("autopilot").expect("autopilot");

    let rendered = action_prompt::render_for_profile(profile, "implementation")
        .expect("implementation should render");
    assert!(
        rendered.contains("# Implementation"),
        "render_for_profile should contain Loom heading"
    );
    assert!(
        !rendered.contains("{{ output }}"),
        "output-specific sections should be resolved for branch profiles"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn builtin_pr_profile_render_for_profile_includes_pr_content() {
    let root = unique_workspace("knots-compat-render-pr");
    let registry = ProfileRegistry::load_for_repo(&root).expect("registry");
    let profile = registry.require("autopilot_with_pr").expect("pr profile");

    let rendered = action_prompt::render_for_profile(profile, "implementation")
        .expect("implementation should render");
    assert!(
        rendered.contains("# Implementation"),
        "PR profile should still have Loom heading"
    );
    assert!(
        rendered.contains("pull request"),
        "PR profile should resolve output-specific PR content"
    );
    let _ = std::fs::remove_dir_all(root);
}

// ── Compat harness peek/claim resolve Loom body for builtin ───

#[test]
fn compat_harness_peek_resolves_loom_body_for_builtin_profile() {
    let root = unique_workspace("knots-compat-peek-loom");
    let db_path = root.join(".knots/cache/state.sqlite");
    let app = crate::app::App::open(db_path.to_str().expect("utf8"), root.clone()).expect("app");

    let knot = app
        .create_knot("Compat peek loom", None, Some("work_item"), None)
        .expect("create");
    let peeked = poll_claim::peek_knot(&app, &knot.id).expect("peek should succeed");

    assert!(
        peeked.skill.contains("# Implementation"),
        "peeked skill should contain Loom heading: got {}",
        &peeked.skill[..peeked.skill.len().min(200)]
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn compat_harness_claim_resolves_loom_body_for_builtin_profile() {
    let root = unique_workspace("knots-compat-claim-loom");
    let db_path = root.join(".knots/cache/state.sqlite");
    let app = crate::app::App::open(db_path.to_str().expect("utf8"), root.clone()).expect("app");

    let knot = app
        .create_knot("Compat claim loom", None, Some("work_item"), None)
        .expect("create");
    let actor = crate::app::StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: None,
        agent_model: None,
        agent_version: None,
    };
    let claimed =
        poll_claim::claim_knot(&app, &knot.id, actor, None).expect("claim should succeed");

    assert!(
        claimed.skill.contains("# Implementation"),
        "claimed skill should contain Loom heading"
    );
    let _ = std::fs::remove_dir_all(root);
}

// ── Custom workflow compat: prompts resolve from Loom body ────

#[test]
fn custom_workflow_compat_prompt_resolves_from_loom_body() {
    let root = unique_workspace("knots-compat-custom-loom");
    install_custom_workflow(&root);

    let installed = InstalledWorkflowRegistry::load(&root).expect("registry");
    let workflow = installed.require_workflow("custom_flow").expect("workflow");
    let prompt = workflow
        .prompt_for_action_state("work")
        .expect("work prompt should exist");

    assert!(
        prompt.body.contains("# Custom Work"),
        "custom workflow prompt body should come from Loom bundle body"
    );
    assert!(
        prompt.accept.contains(&"Built output".to_string()),
        "acceptance criteria should come from Loom bundle"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn custom_workflow_peek_resolves_loom_body_through_compat() {
    let root = unique_workspace("knots-compat-custom-peek");
    install_custom_workflow(&root);
    let db_path = root.join(".knots/cache/state.sqlite");
    let app = crate::app::App::open(db_path.to_str().expect("utf8"), root.clone()).expect("app");

    let knot = app
        .create_knot("Custom compat peek", None, None, None)
        .expect("create");
    let peeked = poll_claim::peek_knot(&app, &knot.id).expect("peek should succeed");

    assert!(
        peeked.skill.contains("# Custom Work"),
        "custom workflow peek should resolve Loom body heading"
    );
    assert!(
        peeked.skill.contains("Ship"),
        "custom workflow peek should contain Loom body text"
    );
    assert!(
        peeked.skill.contains("Built output"),
        "custom workflow peek should include acceptance criteria"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn compat_profiles_coexist_after_custom_workflow_installed() {
    let root = unique_workspace("knots-compat-coexist");
    install_custom_workflow(&root);
    let registry = ProfileRegistry::load_for_repo(&root).expect("registry");

    let builtin = registry.require("autopilot").expect("builtin profile");
    let builtin_impl = builtin
        .prompt_for_action_state("implementation")
        .expect("builtin implementation prompt");
    assert!(
        builtin_impl.contains("# Implementation"),
        "builtin profile should still resolve Loom body after custom install"
    );

    let custom = registry
        .require("custom_flow/autopilot")
        .expect("custom profile");
    let custom_work = custom
        .prompt_for_action_state("work")
        .expect("custom work prompt");
    assert!(
        custom_work.contains("# Custom Work"),
        "custom profile should resolve its own Loom body"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn no_planning_profiles_resolve_implementation_from_loom_body() {
    let root = unique_workspace("knots-compat-no-plan");
    let registry = ProfileRegistry::load_for_repo(&root).expect("registry");

    for profile_id in ["autopilot_no_planning", "autopilot_with_pr_no_planning"] {
        let profile = registry.require(profile_id).expect(profile_id);
        assert!(
            !profile.states.contains(&"planning".to_string()),
            "{profile_id}: no-planning profiles should skip planning state"
        );
        let impl_prompt = profile
            .prompt_for_action_state("implementation")
            .expect("implementation should exist");
        assert!(
            impl_prompt.contains("# Implementation"),
            "{profile_id}: should still resolve implementation from Loom"
        );
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn builtin_prompts_declare_extended_output_target_values() {
    let states_with_output = [
        "implementation",
        "implementation_review",
        "shipment",
        "shipment_review",
    ];
    for state in states_with_output {
        let body = loom_compat_bundle::prompt_body_for_state(state)
            .unwrap_or_else(|| panic!("loom bundle missing {state}"));
        assert!(
            body.contains("branch"),
            "{state}: prompt should mention branch target"
        );
        assert!(
            body.contains("live_deployment"),
            "{state}: prompt should mention live_deployment target"
        );
    }
}
