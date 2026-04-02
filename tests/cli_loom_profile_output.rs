mod cli_dispatch_helpers;

use cli_dispatch_helpers::*;
use serde_json::Value;

// ── Loom assertion helpers ──────────────────────────────────

fn assert_loom_prompt(prompt: &str, state: &str, profile: &str) {
    let heading = match state {
        "planning" => "# Planning",
        "plan_review" => "# Plan Review",
        "implementation" => "# Implementation",
        "implementation_review" => "# Implementation Review",
        "shipment" => "# Shipment",
        "shipment_review" => "# Shipment Review",
        "evaluating" => "# Evaluating",
        _ => panic!("no Loom heading for state: {state}"),
    };
    assert!(
        prompt.contains(heading),
        "REGRESSION: {profile}/{state}: prompt missing Loom heading \
         '{heading}'.\nPrompt excerpt:\n{excerpt}",
        excerpt = &prompt[..prompt.len().min(300)]
    );
}

fn assert_no_unresolved_templates(prompt: &str, state: &str, profile: &str) {
    assert!(
        !prompt.contains("{{ output }}"),
        "REGRESSION: {profile}/{state}: prompt contains unresolved \
         '{{{{ output }}}}' template variable.\n\
         Prompt excerpt:\n{excerpt}",
        excerpt = &prompt[..prompt.len().min(300)]
    );
}

// ── Profile output variant validation ───────────────────────

/// States where the output param differs between autopilot
/// (remote_main) and autopilot_with_pr (pr).
const OUTPUT_SENSITIVE_STATES: &[&str] = &[
    "ready_for_implementation",
    "ready_for_implementation_review",
    "ready_for_shipment",
    "ready_for_shipment_review",
];

#[test]
fn autopilot_claim_resolves_remote_main_output() {
    let root = unique_workspace("knots-e2e-loom-output-rm");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    for queue_state in OUTPUT_SENSITIVE_STATES {
        let created = run_knots(
            &root,
            &db,
            &[
                "new",
                &format!("RM {queue_state}"),
                "--profile",
                "autopilot",
                "--state",
                queue_state,
            ],
        );
        assert_success(&created);
        let knot_id = parse_created_id(&created);

        let claim = run_knots(&root, &db, &["claim", &knot_id, "--json"]);
        assert_success(&claim);
        let json: Value = serde_json::from_slice(&claim.stdout).expect("claim json");
        let prompt = json["prompt"].as_str().expect("prompt should exist");

        assert!(
            prompt.contains("remote_main"),
            "REGRESSION: autopilot/{queue_state}: template should \
             resolve to 'remote_main'.\nPrompt excerpt:\n{excerpt}",
            excerpt = &prompt[..prompt.len().min(500)]
        );
        assert!(
            !prompt.contains("`{{ output }}` = `"),
            "REGRESSION: autopilot/{queue_state}: output-specific \
             conditional markers should be resolved, not raw.\n\
             Prompt excerpt:\n{excerpt}",
            excerpt = &prompt[..prompt.len().min(500)]
        );
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn autopilot_with_pr_claim_resolves_pr_output() {
    let root = unique_workspace("knots-e2e-loom-output-pr");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    for queue_state in OUTPUT_SENSITIVE_STATES {
        let created = run_knots(
            &root,
            &db,
            &[
                "new",
                &format!("PR {queue_state}"),
                "--profile",
                "autopilot_with_pr",
                "--state",
                queue_state,
            ],
        );
        assert_success(&created);
        let knot_id = parse_created_id(&created);

        let claim = run_knots(&root, &db, &["claim", &knot_id, "--json"]);
        assert_success(&claim);
        let json: Value = serde_json::from_slice(&claim.stdout).expect("claim json");
        let prompt = json["prompt"].as_str().expect("prompt should exist");

        assert!(
            prompt.contains("pull request"),
            "REGRESSION: autopilot_with_pr/{queue_state}: PR output \
             should contain 'pull request' content.\n\
             Prompt excerpt:\n{excerpt}",
            excerpt = &prompt[..prompt.len().min(500)]
        );
        assert!(
            !prompt.contains("`{{ output }}` = `"),
            "REGRESSION: autopilot_with_pr/{queue_state}: output \
             conditional markers should be resolved.\n\
             Prompt excerpt:\n{excerpt}",
            excerpt = &prompt[..prompt.len().min(500)]
        );
    }
    let _ = std::fs::remove_dir_all(root);
}

// ── Multiple installed workflows coexist ────────────────────

const CUSTOM_LOOM_BUNDLE: &str = r#"
[workflow]
name = "loom_alt"
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

[states.ready_for_check]
display_name = "Ready for Check"
kind = "queue"

[states.check]
display_name = "Check"
kind = "action"
action_type = "gate"
executor = "human"
prompt = "check"

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

[steps.work_step]
queue = "ready_for_work"
action = "work"

[steps.check_step]
queue = "ready_for_check"
action = "check"

[phases.main]
produce = "work_step"
gate = "check_step"

[profiles.autopilot]
description = "Alt Loom autopilot"
phases = ["main"]
output = "remote_main"

[prompts.work]
accept = ["Alt work delivered"]
body = """
# Alt Loom Work

Perform the alt-loom work task.
"""

[prompts.work.success]
complete = "ready_for_check"

[prompts.work.failure]
blocked = "blocked"

[prompts.check]
accept = ["Alt check passed"]
body = """
# Alt Loom Check

Verify the alt-loom work.
"""

[prompts.check.success]
approved = "done"

[prompts.check.failure]
changes = "ready_for_work"
"#;

fn install_custom_workflow(root: &std::path::Path, db: &std::path::Path) {
    let home = unique_workspace("knots-e2e-loom-multi-home");
    let bundle_path = root.join("loom-alt.toml");
    std::fs::write(&bundle_path, CUSTOM_LOOM_BUNDLE).expect("bundle should write");
    let install = std::process::Command::new(knots_binary())
        .arg("--repo-root")
        .arg(root)
        .arg("--db")
        .arg(db)
        .env("HOME", &home)
        .env("KNOTS_SKIP_DOCTOR_UPGRADE", "1")
        .args([
            "workflow",
            "install",
            bundle_path.to_str().expect("utf8 path"),
            "--set-default=false",
        ])
        .output()
        .expect("install should run");
    assert_success(&install);
}

#[test]
fn builtin_prompts_survive_custom_workflow_install() {
    let root = unique_workspace("knots-e2e-loom-multi");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    install_custom_workflow(&root, &db);

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Builtin after custom",
            "--profile",
            "autopilot",
            "--state",
            "ready_for_implementation",
        ],
    );
    assert_success(&created);
    let knot_id = parse_created_id(&created);

    let claim = run_knots(&root, &db, &["claim", &knot_id, "--json"]);
    assert_success(&claim);
    let json: Value = serde_json::from_slice(&claim.stdout).expect("claim json");
    let prompt = json["prompt"].as_str().expect("prompt should exist");

    assert_loom_prompt(prompt, "implementation", "autopilot");
    assert_no_unresolved_templates(prompt, "implementation", "autopilot");
    assert!(
        !prompt.contains("Alt Loom"),
        "builtin prompt should not bleed custom workflow text"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn custom_workflow_prompts_resolve_independently() {
    let root = unique_workspace("knots-e2e-loom-multi-custom");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    install_custom_workflow(&root, &db);

    let created = run_knots(
        &root,
        &db,
        &["new", "Custom workflow knot", "--workflow", "loom_alt"],
    );
    assert_success(&created);
    let knot_id = parse_created_id(&created);

    let shown = run_knots(&root, &db, &["show", &knot_id, "--json"]);
    assert_success(&shown);
    let shown_json: Value = serde_json::from_slice(&shown.stdout).expect("show json");
    assert_eq!(shown_json["workflow_id"], "loom_alt");

    let claim = run_knots(&root, &db, &["claim", &knot_id, "--json"]);
    assert_success(&claim);
    let json: Value = serde_json::from_slice(&claim.stdout).expect("claim json");
    let prompt = json["prompt"].as_str().expect("prompt should exist");

    assert!(
        prompt.contains("# Alt Loom Work"),
        "custom workflow claim should resolve its own Loom prompt.\n\
         Prompt excerpt:\n{excerpt}",
        excerpt = &prompt[..prompt.len().min(300)]
    );
    assert!(
        prompt.contains("Alt work delivered"),
        "custom workflow claim should include acceptance criteria"
    );
    assert!(
        !prompt.contains("# Implementation"),
        "custom workflow prompt should not contain builtin headings"
    );
    let _ = std::fs::remove_dir_all(root);
}
