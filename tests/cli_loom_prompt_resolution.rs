mod cli_dispatch_helpers;

use cli_dispatch_helpers::*;
use serde_json::Value;

// ── Loom-specific assertion helpers ────────────────────

const LOOM_HEADINGS: &[(&str, &str)] = &[
    ("planning", "# Planning"),
    ("plan_review", "# Plan Review"),
    ("implementation", "# Implementation"),
    ("implementation_review", "# Implementation Review"),
    ("shipment", "# Shipment"),
    ("shipment_review", "# Shipment Review"),
    ("evaluating", "# Evaluating"),
];

const IMPLEMENTATION_LOOM_MARKERS: &[&str] = &[
    "Implement the approved plan on a feature branch.",
    "The expected output artifact is `remote_main`:",
    "a feature branch pushed to remote for direct branch review",
];

const IMPLEMENTATION_STATIC_FALLBACK_MARKERS: &[&str] = &[
    "Run any sanity gates defined in the project or the plan",
    "Add a handoff_capsule to the knot with:",
];

fn loom_heading_for(state: &str) -> &'static str {
    LOOM_HEADINGS
        .iter()
        .find(|(s, _)| *s == state)
        .map(|(_, h)| *h)
        .unwrap_or_else(|| panic!("no Loom heading for state: {state}"))
}

fn assert_loom_prompt(prompt: &str, state: &str, profile: &str) {
    let heading = loom_heading_for(state);
    assert!(
        prompt.contains(heading),
        "REGRESSION: {profile}/{state}: prompt missing Loom heading \
         '{heading}'.\nThis means the prompt may have fallen back to a \
         non-Loom source.\nPrompt excerpt:\n{excerpt}",
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

fn assert_builtin_implementation_prompt(prompt: &str, profile: &str) {
    let context = format!("{profile}/implementation");
    assert_loom_prompt(prompt, "implementation", profile);
    assert_no_unresolved_templates(prompt, "implementation", profile);
    for marker in IMPLEMENTATION_LOOM_MARKERS {
        assert_prompt_contains(prompt, marker, &context);
    }
    for marker in IMPLEMENTATION_STATIC_FALLBACK_MARKERS {
        assert_prompt_not_contains(prompt, marker, &context);
    }
}

const QUEUE_ACTION_PAIRS: &[(&str, &str)] = &[
    ("ready_for_planning", "planning"),
    ("ready_for_plan_review", "plan_review"),
    ("ready_for_implementation", "implementation"),
    ("ready_for_implementation_review", "implementation_review"),
    ("ready_for_shipment", "shipment"),
    ("ready_for_shipment_review", "shipment_review"),
];

// ── Regression matrix: builtin profiles ────────────────────

fn claim_and_assert_loom(
    root: &std::path::Path,
    db: &std::path::Path,
    profile: &str,
    queue_state: &str,
    action_state: &str,
) {
    let created = run_knots(
        root,
        db,
        &[
            "new",
            &format!("{profile} {action_state}"),
            "--profile",
            profile,
            "--state",
            queue_state,
        ],
    );
    assert_success(&created);
    let knot_id = parse_created_id(&created);

    let claim = run_knots(root, db, &["claim", &knot_id, "--json"]);
    assert_success(&claim);
    let json: Value = serde_json::from_slice(&claim.stdout).expect("claim json");
    let prompt = json["prompt"].as_str().expect("prompt should exist");

    assert_loom_prompt(prompt, action_state, profile);
    assert_no_unresolved_templates(prompt, action_state, profile);
    assert_eq!(
        json["state"].as_str().unwrap(),
        action_state,
        "claim should transition to action state"
    );
}

#[test]
fn loom_regression_autopilot_all_action_states() {
    let root = unique_workspace("knots-e2e-loom-autopilot");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    for (queue, action) in QUEUE_ACTION_PAIRS {
        claim_and_assert_loom(&root, &db, "autopilot", queue, action);
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn loom_regression_autopilot_with_pr_all_action_states() {
    let root = unique_workspace("knots-e2e-loom-pr");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    for (queue, action) in QUEUE_ACTION_PAIRS {
        claim_and_assert_loom(&root, &db, "autopilot_with_pr", queue, action);
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn loom_regression_semiauto_all_action_states() {
    let root = unique_workspace("knots-e2e-loom-semi");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    for (queue, action) in QUEUE_ACTION_PAIRS {
        claim_and_assert_loom(&root, &db, "semiauto", queue, action);
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn loom_regression_no_planning_profiles_skip_planning() {
    let root = unique_workspace("knots-e2e-loom-noplan");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let no_plan_pairs: &[(&str, &str)] = &[
        ("ready_for_implementation", "implementation"),
        ("ready_for_implementation_review", "implementation_review"),
        ("ready_for_shipment", "shipment"),
        ("ready_for_shipment_review", "shipment_review"),
    ];

    for profile in ["autopilot_no_planning", "autopilot_with_pr_no_planning"] {
        for (queue, action) in no_plan_pairs {
            claim_and_assert_loom(&root, &db, profile, queue, action);
        }
    }
    let _ = std::fs::remove_dir_all(root);
}

// ── Text output validates Loom headings ────────────────────

#[test]
fn poll_text_output_contains_loom_heading() {
    let root = unique_workspace("knots-e2e-loom-poll-text");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Loom text poll",
            "--profile",
            "autopilot",
            "--state",
            "ready_for_implementation",
        ],
    );
    assert_success(&created);

    let poll = run_knots(&root, &db, &["poll"]);
    assert_success(&poll);
    let stdout = String::from_utf8_lossy(&poll.stdout);

    assert_loom_prompt(&stdout, "implementation", "autopilot");
    assert_no_unresolved_templates(&stdout, "implementation", "autopilot");
    assert!(
        stdout.contains("## Completion"),
        "poll text should include completion command section"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn claim_text_output_contains_loom_heading() {
    let root = unique_workspace("knots-e2e-loom-claim-text");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Loom text claim",
            "--profile",
            "autopilot",
            "--state",
            "ready_for_implementation",
        ],
    );
    assert_success(&created);
    let knot_id = parse_created_id(&created);

    let claim = run_knots(&root, &db, &["claim", &knot_id]);
    assert_success(&claim);
    let stdout = String::from_utf8_lossy(&claim.stdout);

    assert_builtin_implementation_prompt(&stdout, "autopilot");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn default_shipped_workflow_claim_uses_loom_defined_implementation_prompt() {
    let root = unique_workspace("knots-e2e-loom-default-impl");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Default shipped Loom implementation",
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

    assert_builtin_implementation_prompt(prompt, "autopilot");
    let _ = std::fs::remove_dir_all(root);
}

// ── Skill command resolves Loom body ───────────────────────

#[test]
fn skill_command_returns_loom_body_content() {
    let root = unique_workspace("knots-e2e-loom-skill");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    for (_, action_state) in QUEUE_ACTION_PAIRS {
        let skill = run_knots(&root, &db, &["skill", action_state]);
        assert_success(&skill);
        let stdout = String::from_utf8_lossy(&skill.stdout);
        let heading = loom_heading_for(action_state);
        assert!(
            stdout.contains(heading),
            "skill command for {action_state} should return Loom \
             heading '{heading}'.\nGot:\n{excerpt}",
            excerpt = &stdout[..stdout.len().min(300)]
        );
    }
    let _ = std::fs::remove_dir_all(root);
}

// ── Loom acceptance criteria in claim output ───────────────

#[test]
fn claim_json_includes_loom_acceptance_criteria() {
    let root = unique_workspace("knots-e2e-loom-accept");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Acceptance test",
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

    assert!(
        prompt.contains("Working implementation on feature branch"),
        "Loom-defined acceptance should flow through to prompt.\n\
         Prompt excerpt:\n{excerpt}",
        excerpt = &prompt[..prompt.len().min(500)]
    );
    assert!(
        prompt.contains("All tests passing with coverage threshold met"),
        "Loom-defined acceptance criterion should be present"
    );
    let _ = std::fs::remove_dir_all(root);
}
