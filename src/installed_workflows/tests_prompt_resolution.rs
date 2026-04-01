use super::tests_helpers::{unique_workspace, SAMPLE_BUNDLE};
use super::*;

/// Custom bundle with no template variables in prompts, for stable
/// assertions through the claim/poll path where templates are not
/// rendered at profile construction time.
const PLAIN_BUNDLE: &str = r#"
[workflow]
name = "plain_flow"
version = 1
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

[states.ready_for_review]
display_name = "Ready for Review"
kind = "queue"

[states.review]
display_name = "Review"
kind = "action"
action_type = "gate"
executor = "human"
prompt = "review"
output = "note"

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
description = "Plain profile"
phases = ["main"]

[prompts.work]
accept = ["Delivered artifact"]
body = """
Produce the deliverable.
"""

[prompts.work.success]
complete = "ready_for_review"

[prompts.work.failure]
blocked = "blocked"

[prompts.review]
accept = ["Approved artifact"]
body = """
Review the deliverable.
"""

[prompts.review.success]
approved = "done"

[prompts.review.failure]
changes = "ready_for_work"
"#;

// ── Multi-workflow prompt selection ──────────────────────

#[test]
fn switching_workflow_changes_prompt_resolution() {
    let root = unique_workspace("knots-prompt-res-switch");
    let source = root.join("custom-flow.toml");
    std::fs::write(&source, SAMPLE_BUNDLE).expect("write");
    install_bundle(&root, &source).expect("install");

    // Builtin compatibility workflow is selected by default
    let builtin_reg =
        crate::profile::ProfileRegistry::load_for_repo(&root).expect("builtin registry");
    let builtin = builtin_reg.require("autopilot").expect("autopilot");
    assert!(
        builtin
            .prompt_for_action_state("implementation")
            .expect("impl prompt")
            .contains("# Implementation"),
        "builtin profile should have rendered implementation prompt"
    );

    // Switch to custom workflow
    set_current_workflow_selection(&root, "custom_flow", Some(3), Some("autopilot"))
        .expect("select custom");

    let custom_reg =
        crate::profile::ProfileRegistry::load_for_repo(&root).expect("custom registry");
    let custom = custom_reg
        .require("custom_flow/autopilot")
        .expect("custom profile");
    let work_prompt = custom.prompt_for_action_state("work").expect("work prompt");
    // Raw body from bundle (template vars unresolved at this layer)
    assert!(
        work_prompt.contains("Ship"),
        "custom prompt should contain Loom body: {work_prompt}"
    );

    // Builtin profiles still resolve alongside custom
    assert!(
        custom_reg.require("autopilot").is_ok(),
        "builtin profiles should persist after custom install"
    );

    let _ = std::fs::remove_dir_all(root);
}

// ── Custom workflow template parameter substitution ──────

#[test]
fn custom_workflow_template_params_stored_in_prompt_body() {
    let root = unique_workspace("knots-prompt-res-params");
    let source = root.join("custom-flow.toml");
    std::fs::write(&source, SAMPLE_BUNDLE).expect("write");
    install_bundle(&root, &source).expect("install");
    set_current_workflow_selection(&root, "custom_flow", Some(3), Some("autopilot"))
        .expect("select");

    let registry = crate::profile::ProfileRegistry::load_for_repo(&root).expect("registry");
    let profile = registry.require("custom_flow/autopilot").expect("profile");
    let body = profile
        .prompt_for_action_state("work")
        .expect("work prompt");

    // The SAMPLE_BUNDLE uses "Ship {{ output }} output." — the
    // profile stores the raw body; template rendering happens in
    // the compatibility workflow layer and in PromptDefinition::render.
    assert!(
        body.contains("output"),
        "body should reference output param: {body}"
    );

    let _ = std::fs::remove_dir_all(root);
}

// ── Acceptance criteria per action state ─────────────────

#[test]
fn custom_workflow_acceptance_criteria_per_action_state() {
    let root = unique_workspace("knots-prompt-res-accept");
    let source = root.join("custom-flow.toml");
    std::fs::write(&source, SAMPLE_BUNDLE).expect("write");
    install_bundle(&root, &source).expect("install");
    set_current_workflow_selection(&root, "custom_flow", Some(3), Some("autopilot"))
        .expect("select");

    let registry = crate::profile::ProfileRegistry::load_for_repo(&root).expect("registry");
    let profile = registry.require("custom_flow/autopilot").expect("profile");

    assert_eq!(
        profile.acceptance_for_action_state("work"),
        &["Built output".to_string()],
    );
    assert_eq!(
        profile.acceptance_for_action_state("review"),
        &["Reviewed output".to_string()],
    );
    assert!(
        profile
            .acceptance_for_action_state("implementation")
            .is_empty(),
        "non-existent state should have no acceptance"
    );

    let _ = std::fs::remove_dir_all(root);
}

// ── Builtin prompt variants rendered per profile ─────────

#[test]
fn all_builtin_profiles_have_rendered_action_prompts() {
    let root = unique_workspace("knots-prompt-res-builtin-all");
    let registry = InstalledWorkflowRegistry::load(&root).expect("load");
    let workflow = registry
        .require_workflow(COMPATIBILITY_WORKFLOW_ID)
        .expect("compat");

    for (profile_id, profile) in &workflow.profiles {
        for state in &profile.action_states {
            let prompt = profile.prompt_for_action_state(state);
            assert!(prompt.is_some(), "{profile_id} missing prompt for {state}");
            assert!(
                !prompt.unwrap().is_empty(),
                "{profile_id} empty prompt for {state}"
            );
        }
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn builtin_no_planning_profiles_skip_planning_states() {
    let root = unique_workspace("knots-prompt-res-no-plan");
    let registry = InstalledWorkflowRegistry::load(&root).expect("load");
    let workflow = registry
        .require_workflow(COMPATIBILITY_WORKFLOW_ID)
        .expect("compat");

    for pid in [
        "autopilot_no_planning",
        "semiauto_no_planning",
        "autopilot_with_pr_no_planning",
    ] {
        let p = workflow.require_profile(pid).expect("profile");
        assert!(
            !p.action_states.contains(&"planning".to_string()),
            "{pid} should not list planning"
        );
        assert!(
            !p.action_states.contains(&"plan_review".to_string()),
            "{pid} should not list plan_review"
        );
        assert!(
            p.action_states.contains(&"implementation".to_string()),
            "{pid} should still have implementation"
        );
    }
    let _ = std::fs::remove_dir_all(root);
}

// ── Prompt body lookup via WorkflowDefinition ────────────

#[test]
fn workflow_prompt_for_action_state_resolves_via_action_prompts() {
    let root = unique_workspace("knots-prompt-res-wf-lookup");
    let registry = InstalledWorkflowRegistry::load(&root).expect("load");
    let workflow = registry
        .require_workflow(COMPATIBILITY_WORKFLOW_ID)
        .expect("compat");

    for state in [
        "planning",
        "plan_review",
        "implementation",
        "implementation_review",
        "shipment",
        "shipment_review",
        "evaluating",
    ] {
        let def = workflow.prompt_for_action_state(state);
        assert!(def.is_some(), "workflow should have prompt for '{state}'");
        assert_eq!(
            def.unwrap().action_state,
            state,
            "prompt action_state should match"
        );
    }
    let _ = std::fs::remove_dir_all(root);
}

// ── Custom workflow claim/poll end-to-end ────────────────

#[test]
fn custom_workflow_claim_and_poll_resolve_loom_prompts() {
    let root = unique_workspace("knots-prompt-res-e2e");
    let source = root.join("plain-flow.toml");
    std::fs::write(&source, PLAIN_BUNDLE).expect("write");
    install_bundle(&root, &source).expect("install");
    set_current_workflow_selection(&root, "plain_flow", Some(1), Some("autopilot"))
        .expect("select");

    let db_path = root.join(".knots/cache/state.sqlite");
    let app = crate::app::App::open(db_path.to_str().expect("utf8"), root.clone()).expect("app");

    let knot = app
        .create_knot("E2E prompt", None, None, Some("plain_flow/autopilot"))
        .expect("create");

    let peeked = crate::poll_claim::peek_knot(&app, &knot.id).expect("peek");
    assert!(
        peeked.skill.contains("Produce the deliverable."),
        "peek should resolve Loom body"
    );

    let actor = crate::app::StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: Some("test".to_string()),
        agent_model: None,
        agent_version: None,
    };
    let claimed = crate::poll_claim::claim_knot(&app, &knot.id, actor, None).expect("claim");
    assert!(
        claimed.skill.contains("Produce the deliverable."),
        "claim should resolve Loom body"
    );
    assert!(
        claimed.skill.contains("Delivered artifact"),
        "claim should include acceptance criteria"
    );

    let _ = std::fs::remove_dir_all(root);
}
