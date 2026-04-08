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

const IMPLEMENTATION_STATIC_FALLBACK_MARKERS: &[&str] = &[
    "Run any sanity gates defined in the project or the plan",
    "Add a handoff_capsule to the knot with:",
];

fn action_state_for(queue_state: &str) -> &str {
    queue_state
        .strip_prefix("ready_for_")
        .unwrap_or_else(|| panic!("expected queue state, got {queue_state}"))
}

fn assert_branch_output_prompt(prompt: &str, queue_state: &str, profile: &str) {
    let action_state = action_state_for(queue_state);
    let context = format!("{profile}/{action_state}");
    assert_loom_prompt(prompt, action_state, profile);
    assert_no_unresolved_templates(prompt, action_state, profile);
    assert_prompt_contains(prompt, "- artifact: remote_main", &context);

    let expected = match queue_state {
        "ready_for_implementation" => &[
            "The expected output artifact is `remote_main`:",
            "a feature branch pushed to remote for branch review",
        ][..],
        "ready_for_implementation_review" => &["review the branch diff against"][..],
        "ready_for_shipment" => &[
            "merge the feature branch to main",
            "push main after the merge",
        ][..],
        "ready_for_shipment_review" => &["review the code now on main"][..],
        _ => panic!("unexpected output-sensitive state: {queue_state}"),
    };

    for marker in expected {
        assert_prompt_contains(prompt, marker, &context);
    }
    if queue_state == "ready_for_implementation" {
        for marker in IMPLEMENTATION_STATIC_FALLBACK_MARKERS {
            assert_prompt_not_contains(prompt, marker, &context);
        }
    }
}

fn assert_pr_output_prompt(prompt: &str, queue_state: &str, profile: &str) {
    let action_state = action_state_for(queue_state);
    let context = format!("{profile}/{action_state}");
    assert_loom_prompt(prompt, action_state, profile);
    assert_no_unresolved_templates(prompt, action_state, profile);
    assert_prompt_contains(prompt, "- artifact: pr", &context);

    let expected = match queue_state {
        "ready_for_implementation" => &[
            "The expected output artifact is `pr`:",
            "a pull request opened from the feature branch",
        ][..],
        "ready_for_implementation_review" => &["review the pull request diff"][..],
        "ready_for_shipment" => &["merge the approved pull request"][..],
        "ready_for_shipment_review" => &["review the merged pull request"][..],
        _ => panic!("unexpected output-sensitive state: {queue_state}"),
    };

    for marker in expected {
        assert_prompt_contains(prompt, marker, &context);
    }
    if queue_state == "ready_for_implementation" {
        for marker in IMPLEMENTATION_STATIC_FALLBACK_MARKERS {
            assert_prompt_not_contains(prompt, marker, &context);
        }
    }
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

        assert_branch_output_prompt(prompt, queue_state, "autopilot");
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

        assert_pr_output_prompt(prompt, queue_state, "autopilot_with_pr");
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
            "--type",
            "work",
            "--set-default=false",
            bundle_path.to_str().expect("utf8 path"),
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

#[test]
fn builtin_and_custom_workflows_render_distinct_claim_prompts_in_same_repo() {
    let root = unique_workspace("knots-e2e-loom-multi-same-repo");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    install_custom_workflow(&root, &db);

    let builtin = run_knots(
        &root,
        &db,
        &[
            "new",
            "Builtin implementation claim",
            "--profile",
            "autopilot",
            "--state",
            "ready_for_implementation",
        ],
    );
    assert_success(&builtin);
    let builtin_id = parse_created_id(&builtin);

    let custom = run_knots(
        &root,
        &db,
        &["new", "Custom workflow knot", "--workflow", "loom_alt"],
    );
    assert_success(&custom);
    let custom_id = parse_created_id(&custom);

    let builtin_claim = run_knots(&root, &db, &["claim", &builtin_id, "--json"]);
    assert_success(&builtin_claim);
    let builtin_json: Value =
        serde_json::from_slice(&builtin_claim.stdout).expect("builtin claim json");
    let builtin_prompt = builtin_json["prompt"]
        .as_str()
        .expect("builtin prompt should exist");
    assert_branch_output_prompt(builtin_prompt, "ready_for_implementation", "autopilot");
    assert_prompt_not_contains(
        builtin_prompt,
        "# Alt Loom Work",
        "autopilot/implementation",
    );

    let custom_claim = run_knots(&root, &db, &["claim", &custom_id, "--json"]);
    assert_success(&custom_claim);
    let custom_json: Value =
        serde_json::from_slice(&custom_claim.stdout).expect("custom claim json");
    let custom_prompt = custom_json["prompt"]
        .as_str()
        .expect("custom prompt should exist");
    assert_prompt_contains(custom_prompt, "# Alt Loom Work", "loom_alt/work");
    assert_prompt_contains(custom_prompt, "Alt work delivered", "loom_alt/work");
    assert_prompt_not_contains(custom_prompt, "# Implementation", "loom_alt/work");

    let _ = std::fs::remove_dir_all(root);
}
