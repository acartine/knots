mod cli_dispatch_helpers;

use cli_dispatch_helpers::*;
use serde_json::Value;

fn test_core_new_and_ls(root: &std::path::Path, db: &std::path::Path) -> (String, String) {
    let first = run_knots(
        root,
        db,
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
        root,
        db,
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

    let ls = run_knots(root, db, &["ls", "--json"]);
    assert_success(&ls);
    let listed: Value = serde_json::from_slice(&ls.stdout).expect("ls json");
    assert_eq!(listed.as_array().map_or(0, Vec::len), 2);
    (first_id, second_id)
}

fn test_show_state_update(
    root: &std::path::Path,
    db: &std::path::Path,
    first_id: &str,
    second_id: &str,
) {
    let show = run_knots(root, db, &["show", first_id, "--json"]);
    assert_success(&show);
    let shown: Value = serde_json::from_slice(&show.stdout).expect("show json");
    let shown_id = shown
        .get("id")
        .and_then(Value::as_str)
        .expect("shown knot should have an id field");
    assert!(
        shown_id.ends_with(first_id),
        "full id '{shown_id}' should end with '{first_id}'"
    );

    let state = run_knots(root, db, &["state", first_id, "planning"]);
    assert_success(&state);
    let stdout = String::from_utf8_lossy(&state.stdout);
    assert!(stdout.contains("[PLANNING]"), "state: {stdout}");

    let update = run_knots(
        root,
        db,
        &[
            "update",
            first_id,
            "--description",
            "updated description",
            "--add-tag",
            "cli",
            "--status",
            "ready_for_plan_review",
        ],
    );
    assert_success(&update);
    let stdout = String::from_utf8_lossy(&update.stdout);
    assert!(
        stdout.contains("[READY_FOR_PLAN_REVIEW]"),
        "update: {stdout}"
    );

    let edge_add = run_knots(
        root,
        db,
        &["edge", "add", first_id, "blocked_by", second_id],
    );
    assert_success(&edge_add);
    let edge_list = run_knots(root, db, &["edge", "list", first_id, "--json"]);
    assert_success(&edge_list);
    let edges: Value = serde_json::from_slice(&edge_list.stdout).expect("edge list json");
    assert_eq!(edges.as_array().map_or(0, Vec::len), 1);
    assert_success(&run_knots(
        root,
        db,
        &["edge", "remove", first_id, "blocked_by", second_id],
    ));
}

fn test_misc_commands(root: &std::path::Path, db: &std::path::Path, first_id: &str) {
    assert_success(&run_knots(root, db, &["profile", "list", "--json"]));
    assert_success(&run_knots(
        root,
        db,
        &["profile", "show", "autopilot", "--json"],
    ));
    assert_success(&run_knots(root, db, &["fsck", "--json"]));

    let compact_fail = run_knots(root, db, &["compact"]);
    assert_failure(&compact_fail);
    assert!(String::from_utf8_lossy(&compact_fail.stderr)
        .contains("compact currently requires --write-snapshots"));

    assert_success(&run_knots(
        root,
        db,
        &["compact", "--write-snapshots", "--json"],
    ));
    assert_success(&run_knots(root, db, &["rehydrate", first_id, "--json"]));

    let missing = run_knots(root, db, &["show", "missing-id"]);
    assert_failure(&missing);
    assert!(String::from_utf8_lossy(&missing.stderr).contains("not found"));

    let self_unknown = run_knots(root, db, &["self", "update"]);
    assert_failure(&self_unknown);
    assert!(String::from_utf8_lossy(&self_unknown.stderr).contains("unrecognized subcommand"));
}

fn test_skill_and_next(root: &std::path::Path, db: &std::path::Path, first_id: &str) {
    let skill = run_knots(root, db, &["skill", first_id]);
    assert_success(&skill);
    let stdout = String::from_utf8_lossy(&skill.stdout);
    assert!(stdout.contains("# Plan Review"), "skill: {stdout}");

    let next = run_knots(root, db, &["next", first_id, "ready_for_plan_review"]);
    assert_success(&next);
    let stdout = String::from_utf8_lossy(&next.stdout);
    assert!(stdout.contains("updated"), "next: {stdout}");

    let next_missing = run_knots(root, db, &["next", "missing-id", "ready_for_plan_review"]);
    assert_failure(&next_missing);

    let skill_missing = run_knots(root, db, &["skill", "missing-id"]);
    assert_failure(&skill_missing);

    let shipped_knot = run_knots(
        root,
        db,
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
    let next_terminal = run_knots(root, db, &["next", &shipped_id, "shipped"]);
    assert_failure(&next_terminal);
    assert!(String::from_utf8_lossy(&next_terminal.stderr).contains("no next state"));

    let doctor = run_knots(root, db, &["doctor", "--json"]);
    assert_failure(&doctor);
    assert!(String::from_utf8_lossy(&doctor.stderr).contains("doctor found"));
    assert!(
        String::from_utf8_lossy(&doctor.stderr).contains("kno doctor --fix to address these items")
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn core_cli_commands_dispatch_success_and_failure_paths() {
    let root = unique_workspace("knots-cli-dispatch");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let (first_id, second_id) = test_core_new_and_ls(&root, &db);
    test_show_state_update(&root, &db, &first_id, &second_id);
    test_misc_commands(&root, &db, &first_id);
    test_skill_and_next(&root, &db, &first_id);
}

#[test]
fn doctor_without_fix_prints_hint_and_fix_creates_knots_branch() {
    let root = unique_workspace("knots-cli-doctor-fix");
    let _remote = setup_repo_with_remote(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let doctor = run_knots(&root, &db, &["doctor"]);
    assert_success(&doctor);
    assert!(
        String::from_utf8_lossy(&doctor.stderr).contains("kno doctor --fix to address these items")
    );

    let doctor_fix = run_knots(&root, &db, &["doctor", "--fix"]);
    assert_success(&doctor_fix);
    assert!(!String::from_utf8_lossy(&doctor_fix.stderr)
        .contains("kno doctor --fix to address these items"));

    let knots_remote = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["ls-remote", "--exit-code", "--heads", "origin", "knots"])
        .output()
        .expect("git ls-remote should run");
    assert!(
        knots_remote.status.success(),
        "expected origin/knots after doctor --fix, stderr: {}",
        String::from_utf8_lossy(&knots_remote.stderr)
    );

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
fn init_command_bootstraps_from_existing_remote_branch() {
    let root = unique_workspace("knots-cli-init-existing");
    let remote = setup_repo_with_remote(&root);
    let db = root.join(".knots/cache/state.sqlite");

    assert_success(&run_knots(&root, &db, &["init"]));
    let created = run_knots(
        &root,
        &db,
        &["new", "Shared knot", "--desc", "available in clone"],
    );
    assert_success(&created);
    let knot_id = parse_created_id(&created);
    assert_success(&run_knots(&root, &db, &["sync"]));

    let clone = unique_workspace("knots-cli-init-existing-clone");
    let clone_output = std::process::Command::new("git")
        .arg("clone")
        .arg(&remote)
        .arg(&clone)
        .output()
        .expect("git clone should run");
    assert!(
        clone_output.status.success(),
        "git clone failed: {}",
        String::from_utf8_lossy(&clone_output.stderr)
    );
    run_git(&clone, &["config", "user.email", "knots@example.com"]);
    run_git(&clone, &["config", "user.name", "Knots Test"]);

    let clone_db = clone.join(".knots/cache/state.sqlite");
    let init = run_knots(&clone, &clone_db, &["init"]);
    assert_success(&init);
    let init_stdout = String::from_utf8_lossy(&init.stdout);
    assert!(
        init_stdout.contains("pulling knots from remote"),
        "clone init: {init_stdout}"
    );

    let show = run_knots(&clone, &clone_db, &["show", &knot_id]);
    assert_success(&show);
    let show_stdout = String::from_utf8_lossy(&show.stdout);
    assert!(
        show_stdout.contains("Shared knot"),
        "clone show: {show_stdout}"
    );

    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(clone);
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
    let stdout = String::from_utf8_lossy(&created.stdout);
    assert!(stdout.contains("[READY_FOR_PLANNING]"), "new: {stdout}");
    let knot_id = parse_created_id(&created);

    assert_success(&run_knots(&root, &db, &["ls"]));
    assert_success(&run_knots(&root, &db, &["show", &knot_id]));
    let profile_list = run_knots(&root, &db, &["profile", "list"]);
    assert_success(&profile_list);
    let stdout = String::from_utf8_lossy(&profile_list.stdout);
    assert!(stdout.contains("(default)"), "profile list: {stdout}");
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
fn completions_command_generates_bash_output() {
    let root = unique_workspace("knots-cli-completions");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let result = run_knots(&root, &db, &["completions", "bash"]);
    assert_success(&result);
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(!stdout.is_empty());
    assert!(stdout.contains("kno"), "completions: {stdout}");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn new_fast_flag_and_q_command_use_quick_profile() {
    let root = unique_workspace("knots-cli-new-fast");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let fast = run_knots(&root, &db, &["new", "Fast task", "-f"]);
    assert_success(&fast);
    let stdout = String::from_utf8_lossy(&fast.stdout);
    assert!(
        stdout.contains("[READY_FOR_IMPLEMENTATION]"),
        "fast: {stdout}"
    );

    let q = run_knots(&root, &db, &["q", "Quick task"]);
    assert_success(&q);
    let stdout = String::from_utf8_lossy(&q.stdout);
    assert!(stdout.contains("[READY_FOR_IMPLEMENTATION]"), "q: {stdout}");

    let _ = std::fs::remove_dir_all(root);
}
