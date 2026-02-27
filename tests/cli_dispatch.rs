use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use uuid::Uuid;

fn unique_workspace(prefix: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&path).expect("workspace should be creatable");
    path
}

fn run_git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
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

fn setup_repo(root: &Path) {
    run_git(root, &["init"]);
    run_git(root, &["config", "user.email", "knots@example.com"]);
    run_git(root, &["config", "user.name", "Knots Test"]);
    std::fs::write(root.join("README.md"), "# knots\n").expect("readme should be writable");
    run_git(root, &["add", "README.md"]);
    run_git(root, &["commit", "-m", "init"]);
    run_git(root, &["branch", "-M", "main"]);
}

fn setup_repo_with_remote(root: &Path) -> PathBuf {
    setup_repo(root);
    let remote = root.join("remote.git");
    run_git(
        root,
        &["init", "--bare", remote.to_str().expect("utf8 path")],
    );
    run_git(
        root,
        &[
            "remote",
            "add",
            "origin",
            remote.to_str().expect("utf8 path"),
        ],
    );
    remote
}

fn run_knots(repo_root: &Path, db_path: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_knots"))
        .arg("--repo-root")
        .arg(repo_root)
        .arg("--db")
        .arg(db_path)
        .args(args)
        .output()
        .expect("knots command should run")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "expected success but failed.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_failure(output: &Output) {
    assert!(
        !output.status.success(),
        "expected failure but command succeeded.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn parse_created_id(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .split_whitespace()
        .nth(1)
        .expect("created output should include knot id")
        .to_string()
}

#[test]
fn core_cli_commands_dispatch_success_and_failure_paths() {
    let root = unique_workspace("knots-cli-dispatch");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let first = run_knots(
        &root,
        &db,
        &[
            "new",
            "First knot",
            "--profile",
            "autopilot",
            "--state",
            "idea",
        ],
    );
    assert_success(&first);
    let first_id = parse_created_id(&first);

    let second = run_knots(
        &root,
        &db,
        &[
            "new",
            "Second knot",
            "--profile",
            "autopilot",
            "--state",
            "idea",
        ],
    );
    assert_success(&second);
    let second_id = parse_created_id(&second);

    let ls = run_knots(&root, &db, &["ls", "--json"]);
    assert_success(&ls);
    let listed: Value = serde_json::from_slice(&ls.stdout).expect("ls should emit json");
    assert_eq!(listed.as_array().map_or(0, Vec::len), 2);

    let show = run_knots(&root, &db, &["show", &first_id, "--json"]);
    assert_success(&show);
    let shown: Value = serde_json::from_slice(&show.stdout).expect("show should emit json");
    let shown_id = shown
        .get("id")
        .and_then(Value::as_str)
        .expect("shown knot should have an id field");
    assert!(
        shown_id.ends_with(&first_id),
        "full id '{shown_id}' should end with display id '{first_id}'"
    );

    let state = run_knots(&root, &db, &["state", &first_id, "planning"]);
    assert_success(&state);
    let state_stdout = String::from_utf8_lossy(&state.stdout);
    assert!(
        state_stdout.contains("[PLANNING]"),
        "kno state output should contain uppercase bracketed state: {state_stdout}"
    );

    let update = run_knots(
        &root,
        &db,
        &[
            "update",
            &first_id,
            "--description",
            "updated description",
            "--add-tag",
            "cli",
            "--status",
            "ready_for_plan_review",
        ],
    );
    assert_success(&update);
    let update_stdout = String::from_utf8_lossy(&update.stdout);
    assert!(
        update_stdout.contains("[READY_FOR_PLAN_REVIEW]"),
        "kno update output should contain uppercase bracketed state: {update_stdout}"
    );

    let edge_add = run_knots(
        &root,
        &db,
        &["edge", "add", &first_id, "blocked_by", &second_id],
    );
    assert_success(&edge_add);
    let edge_list = run_knots(&root, &db, &["edge", "list", &first_id, "--json"]);
    assert_success(&edge_list);
    let edges: Value = serde_json::from_slice(&edge_list.stdout).expect("edge list should be json");
    assert_eq!(edges.as_array().map_or(0, Vec::len), 1);
    let edge_remove = run_knots(
        &root,
        &db,
        &["edge", "remove", &first_id, "blocked_by", &second_id],
    );
    assert_success(&edge_remove);

    assert_success(&run_knots(&root, &db, &["profile", "list", "--json"]));
    assert_success(&run_knots(
        &root,
        &db,
        &["profile", "show", "autopilot", "--json"],
    ));
    assert_success(&run_knots(&root, &db, &["fsck", "--json"]));

    let compact_fail = run_knots(&root, &db, &["compact"]);
    assert_failure(&compact_fail);
    assert!(String::from_utf8_lossy(&compact_fail.stderr)
        .contains("compact currently requires --write-snapshots"));

    assert_success(&run_knots(
        &root,
        &db,
        &["compact", "--write-snapshots", "--json"],
    ));
    assert_success(&run_knots(&root, &db, &["rehydrate", &first_id, "--json"]));

    let missing = run_knots(&root, &db, &["show", "missing-id"]);
    assert_failure(&missing);
    assert!(String::from_utf8_lossy(&missing.stderr).contains("not found"));

    let self_unknown = run_knots(&root, &db, &["self", "update"]);
    assert_failure(&self_unknown);
    assert!(String::from_utf8_lossy(&self_unknown.stderr).contains("unrecognized subcommand"));

    // first_id is in ready_for_plan_review, so skill resolves plan_review
    let skill = run_knots(&root, &db, &["skill", &first_id]);
    assert_success(&skill);
    let skill_stdout = String::from_utf8_lossy(&skill.stdout);
    assert!(
        skill_stdout.contains("# Plan Review"),
        "skill should print plan review markdown: {skill_stdout}"
    );

    // next advances ready_for_plan_review -> plan_review
    let next = run_knots(&root, &db, &["next", &first_id]);
    assert_success(&next);
    let next_stdout = String::from_utf8_lossy(&next.stdout);
    assert!(
        next_stdout.contains("updated"),
        "next should print updated: {next_stdout}"
    );

    let next_missing = run_knots(&root, &db, &["next", "missing-id"]);
    assert_failure(&next_missing);

    let skill_missing = run_knots(&root, &db, &["skill", "missing-id"]);
    assert_failure(&skill_missing);

    // Terminal state has no next - test error path
    let shipped_knot = run_knots(
        &root,
        &db,
        &[
            "new",
            "Shipped knot",
            "--profile",
            "autopilot",
            "--state",
            "shipped",
        ],
    );
    assert_success(&shipped_knot);
    let shipped_id = parse_created_id(&shipped_knot);
    let next_terminal = run_knots(&root, &db, &["next", &shipped_id]);
    assert_failure(&next_terminal);
    assert!(String::from_utf8_lossy(&next_terminal.stderr).contains("no next state"));

    let doctor = run_knots(&root, &db, &["doctor", "--json"]);
    assert_failure(&doctor);
    assert!(String::from_utf8_lossy(&doctor.stderr).contains("doctor found"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn init_and_uninit_commands_work_with_remote_origin() {
    let root = unique_workspace("knots-cli-init");
    let _remote = setup_repo_with_remote(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let init = run_knots(&root, &db, &["init"]);
    assert_success(&init);
    assert!(String::from_utf8_lossy(&init.stdout).contains("kno init completed"));
    assert!(root.join(".knots").exists());
    let gitignore = std::fs::read_to_string(root.join(".gitignore"))
        .expect(".gitignore should exist after init");
    assert!(gitignore.lines().any(|line| line.trim() == "/.knots/"));

    let uninit = run_knots(&root, &db, &["uninit"]);
    assert_success(&uninit);
    assert!(String::from_utf8_lossy(&uninit.stdout).contains("kno uninit completed"));
    assert!(!root.join(".knots").exists());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cli_dispatch_covers_non_json_paths_and_remote_sync_commands() {
    let root = unique_workspace("knots-cli-dispatch-non-json");
    let _remote = setup_repo_with_remote(&root);
    let db = root.join(".knots/cache/state.sqlite");

    assert_success(&run_knots(&root, &db, &["init-remote"]));
    let gitignore = std::fs::read_to_string(root.join(".gitignore"))
        .expect(".gitignore should exist after init-remote");
    assert!(gitignore.lines().any(|line| line.trim() == "/.knots/"));

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Non-json knot",
            "--profile",
            "autopilot",
            "--state",
            "idea",
        ],
    );
    assert_success(&created);
    let created_stdout = String::from_utf8_lossy(&created.stdout);
    assert!(
        created_stdout.contains("[READY_FOR_PLANNING]"),
        "kno new should format state in uppercase like kno ls: {created_stdout}"
    );
    let knot_id = parse_created_id(&created);

    assert_success(&run_knots(&root, &db, &["ls"]));
    assert_success(&run_knots(&root, &db, &["show", &knot_id]));
    let profile_list = run_knots(&root, &db, &["profile", "list"]);
    assert_success(&profile_list);
    let profile_list_stdout = String::from_utf8_lossy(&profile_list.stdout);
    assert!(
        profile_list_stdout.contains("(default)"),
        "profile list should show (default) marker: {profile_list_stdout}"
    );
    assert_success(&run_knots(&root, &db, &["profile", "show", "autopilot"]));
    assert_success(&run_knots(&root, &db, &["fsck"]));
    assert_success(&run_knots(&root, &db, &["rehydrate", &knot_id]));

    assert_success(&run_knots(&root, &db, &["edge", "list", &knot_id]));

    let second = run_knots(
        &root,
        &db,
        &[
            "new",
            "Second non-json knot",
            "--profile",
            "autopilot",
            "--state",
            "idea",
        ],
    );
    assert_success(&second);
    let second_id = parse_created_id(&second);

    assert_success(&run_knots(
        &root,
        &db,
        &["edge", "add", &knot_id, "blocked_by", &second_id],
    ));
    assert_success(&run_knots(&root, &db, &["edge", "list", &knot_id]));
    assert_success(&run_knots(
        &root,
        &db,
        &["edge", "remove", &knot_id, "blocked_by", &second_id],
    ));

    let self_unknown = run_knots(&root, &db, &["self", "update"]);
    assert_failure(&self_unknown);
    assert!(String::from_utf8_lossy(&self_unknown.stderr).contains("unrecognized subcommand"));

    assert_success(&run_knots(&root, &db, &["push"]));
    assert_success(&run_knots(&root, &db, &["pull"]));
    assert_success(&run_knots(&root, &db, &["sync"]));
    assert_success(&run_knots(&root, &db, &["cold", "sync"]));
    assert_success(&run_knots(&root, &db, &["cold", "search", "no-match-term"]));

    assert_success(&run_knots(&root, &db, &["perf", "--iterations", "1"]));

    let doctor = run_knots(&root, &db, &["doctor"]);
    assert_success(&doctor);
    assert!(String::from_utf8_lossy(&doctor.stdout).contains("lock_health"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cli_dispatch_covers_json_branches_and_cold_search_results() {
    let root = unique_workspace("knots-cli-json-branches");
    let _remote = setup_repo_with_remote(&root);
    let db = root.join(".knots/cache/state.sqlite");

    assert_success(&run_knots(&root, &db, &["init-remote"]));

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Cold candidate",
            "--profile",
            "autopilot",
            "--state",
            "shipped",
        ],
    );
    assert_success(&created);
    let knot_id = parse_created_id(&created);

    assert_success(&run_knots(
        &root,
        &db,
        &[
            "update",
            &knot_id,
            "--description",
            "cold description",
            "--add-note",
            "note body",
            "--note-username",
            "acartine",
            "--note-datetime",
            "2026-02-25T10:00:00Z",
            "--note-agentname",
            "codex",
            "--note-model",
            "gpt-5",
            "--note-version",
            "1",
            "--add-handoff-capsule",
            "handoff body",
            "--handoff-username",
            "acartine",
            "--handoff-datetime",
            "2026-02-25T10:05:00Z",
            "--handoff-agentname",
            "codex",
            "--handoff-model",
            "gpt-5",
            "--handoff-version",
            "1",
        ],
    ));

    let push = run_knots(&root, &db, &["push", "--json"]);
    assert_success(&push);
    let _push_json: Value = serde_json::from_slice(&push.stdout).expect("push json should parse");

    let pull = run_knots(&root, &db, &["pull", "--json"]);
    assert_success(&pull);
    let _pull_json: Value = serde_json::from_slice(&pull.stdout).expect("pull json should parse");

    let sync = run_knots(&root, &db, &["sync", "--json"]);
    assert_success(&sync);
    let _sync_json: Value = serde_json::from_slice(&sync.stdout).expect("sync json should parse");

    let perf = run_knots(&root, &db, &["perf", "--iterations", "1", "--json"]);
    assert_success(&perf);
    let _perf_json: Value = serde_json::from_slice(&perf.stdout).expect("perf json should parse");

    let compact = run_knots(&root, &db, &["compact", "--write-snapshots"]);
    assert_success(&compact);

    let cold_sync = run_knots(&root, &db, &["cold", "sync", "--json"]);
    assert_success(&cold_sync);
    let _cold_sync_json: Value =
        serde_json::from_slice(&cold_sync.stdout).expect("cold sync json should parse");

    let cold_search_json = run_knots(&root, &db, &["cold", "search", "Cold", "--json"]);
    assert_success(&cold_search_json);
    let cold_matches: Value =
        serde_json::from_slice(&cold_search_json.stdout).expect("cold search json should parse");
    assert!(cold_matches.as_array().is_some());

    let cold_search_text = run_knots(&root, &db, &["cold", "search", "Cold"]);
    assert_success(&cold_search_text);
    assert!(String::from_utf8_lossy(&cold_search_text.stdout).contains("Cold"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn completions_command_generates_bash_output() {
    let root = unique_workspace("knots-cli-completions");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let result = run_knots(&root, &db, &["completions", "bash"]);
    assert_success(&result);
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(!stdout.is_empty(), "completions output should be non-empty");
    assert!(
        stdout.contains("kno"),
        "completions should reference kno: {stdout}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn new_fast_flag_and_q_command_use_quick_profile() {
    let root = unique_workspace("knots-cli-new-fast");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    // kno new -f should use the default quick profile (autopilot_no_planning)
    let fast = run_knots(&root, &db, &["new", "Fast task", "-f"]);
    assert_success(&fast);
    let fast_stdout = String::from_utf8_lossy(&fast.stdout);
    // autopilot_no_planning starts at ready_for_implementation
    assert!(
        fast_stdout.contains("[READY_FOR_IMPLEMENTATION]"),
        "kno new -f should use quick profile: {fast_stdout}"
    );

    // kno q should also use the quick profile
    let q = run_knots(&root, &db, &["q", "Quick task"]);
    assert_success(&q);
    let q_stdout = String::from_utf8_lossy(&q.stdout);
    assert!(
        q_stdout.contains("[READY_FOR_IMPLEMENTATION]"),
        "kno q should use quick profile: {q_stdout}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn poll_returns_highest_priority_agent_owned_knot() {
    let root = unique_workspace("knots-cli-poll");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    // Create two knots with different priorities
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

    // Poll should return the high-priority knot
    let poll = run_knots(&root, &db, &["poll", "--json"]);
    assert_success(&poll);
    let poll_json: Value = serde_json::from_slice(&poll.stdout).expect("poll json should parse");
    assert_eq!(poll_json["title"], "High priority");
    assert!(poll_json["prompt"]
        .as_str()
        .unwrap()
        .contains("# High priority"));

    // Poll text output should contain the skill
    let poll_text = run_knots(&root, &db, &["poll"]);
    assert_success(&poll_text);
    let stdout = String::from_utf8_lossy(&poll_text.stdout);
    assert!(stdout.contains("# High priority"), "poll text: {stdout}");
    assert!(
        stdout.contains("# Implementation"),
        "poll should include skill: {stdout}"
    );
    assert!(
        stdout.contains("## Completion"),
        "poll should include completion: {stdout}"
    );
    assert!(
        stdout.contains("kno next"),
        "poll completion should use kno next: {stdout}"
    );
    assert!(
        stdout.contains("--actor-kind agent"),
        "poll completion should include actor kind: {stdout}"
    );

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

    // Poll for implementation should find nothing
    let poll_impl = run_knots(&root, &db, &["poll", "implementation"]);
    assert_failure(&poll_impl);

    // Poll for planning should find the knot
    let poll_plan = run_knots(&root, &db, &["poll", "planning"]);
    assert_success(&poll_plan);
    let stdout = String::from_utf8_lossy(&poll_plan.stdout);
    assert!(stdout.contains("Plan me"), "poll planning: {stdout}");

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
        "empty poll: {stderr}"
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

    // Claim should transition and return prompt
    let claim = run_knots(
        &root,
        &db,
        &["claim", &knot_id, "--agent-name", "test-agent"],
    );
    assert_success(&claim);
    let stdout = String::from_utf8_lossy(&claim.stdout);
    assert!(stdout.contains("# Claim me"), "claim text: {stdout}");
    assert!(
        stdout.contains("# Implementation"),
        "claim should include skill: {stdout}"
    );
    assert!(
        stdout.contains("kno next"),
        "claim completion should use kno next: {stdout}"
    );
    assert!(
        stdout.contains("--actor-kind agent"),
        "claim completion should include actor kind: {stdout}"
    );

    // Show should confirm state changed to implementation
    let show = run_knots(&root, &db, &["show", &knot_id, "--json"]);
    assert_success(&show);
    let shown: Value = serde_json::from_slice(&show.stdout).unwrap();
    assert_eq!(shown["state"], "implementation");

    // Second claim should fail (already in action state)
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

    let next_bad = run_knots(&root, &db, &["next", &knot_bad_id, "--actor-kind", "robot"]);
    assert_failure(&next_bad);
    assert!(
        String::from_utf8_lossy(&next_bad.stderr)
            .contains("--actor-kind must be one of: human, agent"),
        "next invalid actor-kind should be rejected"
    );

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
    assert!(stdout.contains("# Grab me"), "poll --claim: {stdout}");
    assert!(stdout.contains("# Implementation"), "skill: {stdout}");

    // Verify the knot is now in action state
    let show = run_knots(&root, &db, &["show", &knot_id, "--json"]);
    assert_success(&show);
    let shown: Value = serde_json::from_slice(&show.stdout).unwrap();
    assert_eq!(shown["state"], "implementation");

    // Queue should now be empty
    let poll_empty = run_knots(&root, &db, &["poll"]);
    assert_failure(&poll_empty);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn poll_filters_human_owned_stages() {
    let root = unique_workspace("knots-cli-poll-human");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    // semiauto profile: plan_review is human-owned
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

    // Default poll (agent owner) should not find it
    let poll_agent = run_knots(&root, &db, &["poll"]);
    assert_failure(&poll_agent);

    // Poll with --owner human should find it
    let poll_human = run_knots(&root, &db, &["poll", "--owner", "human"]);
    assert_success(&poll_human);
    let stdout = String::from_utf8_lossy(&poll_human.stdout);
    assert!(stdout.contains("Human gate"), "human poll: {stdout}");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn skill_command_accepts_state_name_as_fallback() {
    let root = unique_workspace("knots-cli-skill-state");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    // Lowercase state name
    let skill_planning = run_knots(&root, &db, &["skill", "planning"]);
    assert_success(&skill_planning);
    let skill_stdout = String::from_utf8_lossy(&skill_planning.stdout);
    assert!(
        skill_stdout.contains("# Planning"),
        "kno skill planning should print planning markdown: {skill_stdout}"
    );

    // Uppercase state name (case-insensitive)
    let skill_upper = run_knots(&root, &db, &["skill", "PLANNING"]);
    assert_success(&skill_upper);
    let upper_stdout = String::from_utf8_lossy(&skill_upper.stdout);
    assert!(
        upper_stdout.contains("# Planning"),
        "kno skill PLANNING should work case-insensitively: {upper_stdout}"
    );

    // Nonsense state name should fail
    let skill_nonsense = run_knots(&root, &db, &["skill", "nonsense"]);
    assert_failure(&skill_nonsense);
    assert!(
        String::from_utf8_lossy(&skill_nonsense.stderr)
            .contains("is not a knot id or skill state name"),
        "skill nonsense should produce helpful error"
    );

    let _ = std::fs::remove_dir_all(root);
}
