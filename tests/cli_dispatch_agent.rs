mod cli_dispatch_helpers;

use cli_dispatch_helpers::*;
use serde_json::Value;

#[test]
fn poll_returns_highest_priority_agent_owned_knot() {
    let root = unique_workspace("knots-cli-poll");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let low_prio = run_knots(
        &root,
        &db,
        &[
            "new",
            "Low priority",
            "--profile",
            "autopilot",
            "--state",
            "ready_for_implementation",
        ],
    );
    assert_success(&low_prio);
    let low_id = parse_created_id(&low_prio);
    assert_success(&run_knots(
        &root,
        &db,
        &["update", &low_id, "--priority", "3"],
    ));

    let high_prio = run_knots(
        &root,
        &db,
        &[
            "new",
            "High priority",
            "--profile",
            "autopilot",
            "--state",
            "ready_for_implementation",
        ],
    );
    assert_success(&high_prio);
    let high_id = parse_created_id(&high_prio);
    assert_success(&run_knots(
        &root,
        &db,
        &["update", &high_id, "--priority", "1"],
    ));

    let poll = run_knots(&root, &db, &["poll", "--json"]);
    assert_success(&poll);
    let json: Value = serde_json::from_slice(&poll.stdout).expect("poll json");
    assert_eq!(json["title"], "High priority");
    assert!(json["prompt"].as_str().unwrap().contains("# High priority"));

    let poll_text = run_knots(&root, &db, &["poll"]);
    assert_success(&poll_text);
    let stdout = String::from_utf8_lossy(&poll_text.stdout);
    assert!(stdout.contains("# High priority"), "poll: {stdout}");
    assert!(stdout.contains("# Implementation"), "skill: {stdout}");
    assert!(stdout.contains("## Completion"), "completion: {stdout}");
    assert!(stdout.contains("kno next"), "next cmd: {stdout}");
    assert!(stdout.contains("--actor-kind agent"), "actor: {stdout}");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn poll_with_stage_filter() {
    let root = unique_workspace("knots-cli-poll-stage");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Plan me",
            "--profile",
            "autopilot",
            "--state",
            "ready_for_planning",
        ],
    );
    assert_success(&created);

    let poll_impl = run_knots(&root, &db, &["poll", "implementation"]);
    assert_failure(&poll_impl);

    let poll_plan = run_knots(&root, &db, &["poll", "planning"]);
    assert_success(&poll_plan);
    let stdout = String::from_utf8_lossy(&poll_plan.stdout);
    assert!(stdout.contains("Plan me"), "planning: {stdout}");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn poll_returns_nothing_when_queue_empty() {
    let root = unique_workspace("knots-cli-poll-empty");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let poll = run_knots(&root, &db, &["poll"]);
    assert_failure(&poll);
    let stderr = String::from_utf8_lossy(&poll.stderr);
    assert!(
        stderr.contains("no claimable knots found"),
        "empty: {stderr}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn claim_transitions_and_returns_prompt() {
    let root = unique_workspace("knots-cli-claim");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Claim me",
            "--profile",
            "autopilot",
            "--state",
            "ready_for_implementation",
        ],
    );
    assert_success(&created);
    let knot_id = parse_created_id(&created);

    let claim = run_knots(
        &root,
        &db,
        &["claim", &knot_id, "--agent-name", "test-agent"],
    );
    assert_success(&claim);
    let stdout = String::from_utf8_lossy(&claim.stdout);
    assert!(stdout.contains("# Claim me"), "claim: {stdout}");
    assert!(stdout.contains("# Implementation"), "skill: {stdout}");
    assert!(stdout.contains("kno next"), "next: {stdout}");
    assert!(stdout.contains("--actor-kind agent"), "actor: {stdout}");

    let show = run_knots(&root, &db, &["show", &knot_id, "--json"]);
    assert_success(&show);
    let shown: Value = serde_json::from_slice(&show.stdout).unwrap();
    assert_eq!(shown["state"], "implementation");

    let claim2 = run_knots(&root, &db, &["claim", &knot_id]);
    assert_failure(&claim2);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn claim_json_output() {
    let root = unique_workspace("knots-cli-claim-json");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "JSON claim",
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
    let json: Value = serde_json::from_slice(&claim.stdout).expect("claim json should parse");
    assert_eq!(json["title"], "JSON claim");
    assert!(json["prompt"]
        .as_str()
        .unwrap()
        .contains("# Implementation"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn next_accepts_actor_metadata_and_validates_actor_kind() {
    let root = unique_workspace("knots-cli-next-actor");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Next actor metadata",
            "--profile",
            "autopilot",
            "--state",
            "ready_for_plan_review",
        ],
    );
    assert_success(&created);
    let knot_id = parse_created_id(&created);

    let next_ok = run_knots(
        &root,
        &db,
        &[
            "next",
            &knot_id,
            "ready_for_plan_review",
            "--actor-kind",
            "agent",
            "--agent-name",
            "codex",
            "--agent-model",
            "gpt-5",
            "--agent-version",
            "1.0",
        ],
    );
    assert_success(&next_ok);

    let created_bad = run_knots(
        &root,
        &db,
        &[
            "new",
            "Next actor invalid",
            "--profile",
            "autopilot",
            "--state",
            "ready_for_plan_review",
        ],
    );
    assert_success(&created_bad);
    let knot_bad_id = parse_created_id(&created_bad);

    let next_bad = run_knots(
        &root,
        &db,
        &[
            "next",
            &knot_bad_id,
            "ready_for_plan_review",
            "--actor-kind",
            "robot",
        ],
    );
    assert_failure(&next_bad);
    assert!(String::from_utf8_lossy(&next_bad.stderr)
        .contains("--actor-kind must be one of: human, agent"),);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn next_json_flag_emits_structured_output() {
    let root = unique_workspace("knots-cli-next-json");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Next json test",
            "--profile",
            "autopilot",
            "--state",
            "ready_for_plan_review",
        ],
    );
    assert_success(&created);
    let knot_id = parse_created_id(&created);

    let next_json = run_knots(
        &root,
        &db,
        &["next", &knot_id, "ready_for_plan_review", "--json"],
    );
    assert_success(&next_json);
    let stdout = String::from_utf8_lossy(&next_json.stdout);
    let parsed: Value = serde_json::from_str(&stdout).expect("next --json should emit valid JSON");
    assert_eq!(parsed["previous_state"], "ready_for_plan_review");
    assert_eq!(parsed["state"], "plan_review");
    assert!(parsed["id"].is_string());
    assert_eq!(parsed["owner_kind"], "agent");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn poll_claim_flag_atomically_grabs() {
    let root = unique_workspace("knots-cli-poll-claim");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Grab me",
            "--profile",
            "autopilot",
            "--state",
            "ready_for_implementation",
        ],
    );
    assert_success(&created);
    let knot_id = parse_created_id(&created);

    let poll_claim = run_knots(&root, &db, &["poll", "--claim"]);
    assert_success(&poll_claim);
    let stdout = String::from_utf8_lossy(&poll_claim.stdout);
    assert!(stdout.contains("# Grab me"), "claim: {stdout}");
    assert!(stdout.contains("# Implementation"), "skill: {stdout}");

    let show = run_knots(&root, &db, &["show", &knot_id, "--json"]);
    assert_success(&show);
    let shown: Value = serde_json::from_slice(&show.stdout).unwrap();
    assert_eq!(shown["state"], "implementation");

    let poll_empty = run_knots(&root, &db, &["poll"]);
    assert_failure(&poll_empty);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn poll_filters_human_owned_stages() {
    let root = unique_workspace("knots-cli-poll-human");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Human gate",
            "--profile",
            "semiauto",
            "--state",
            "ready_for_plan_review",
        ],
    );
    assert_success(&created);

    let poll_agent = run_knots(&root, &db, &["poll"]);
    assert_failure(&poll_agent);

    let poll_human = run_knots(&root, &db, &["poll", "--owner", "human"]);
    assert_success(&poll_human);
    let stdout = String::from_utf8_lossy(&poll_human.stdout);
    assert!(stdout.contains("Human gate"), "human: {stdout}");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn skill_command_accepts_state_name_as_fallback() {
    let root = unique_workspace("knots-cli-skill-state");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let skill = run_knots(&root, &db, &["skill", "planning"]);
    assert_success(&skill);
    let stdout = String::from_utf8_lossy(&skill.stdout);
    assert!(stdout.contains("# Planning"), "planning: {stdout}");

    let skill_upper = run_knots(&root, &db, &["skill", "PLANNING"]);
    assert_success(&skill_upper);
    let stdout = String::from_utf8_lossy(&skill_upper.stdout);
    assert!(stdout.contains("# Planning"), "upper: {stdout}");

    let skill_nonsense = run_knots(&root, &db, &["skill", "nonsense"]);
    assert_failure(&skill_nonsense);
    assert!(String::from_utf8_lossy(&skill_nonsense.stderr)
        .contains("is not a knot id or skill state name"));

    let _ = std::fs::remove_dir_all(root);
}
