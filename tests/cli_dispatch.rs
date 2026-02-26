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
    assert_eq!(
        shown.get("id").and_then(Value::as_str),
        Some(first_id.as_str())
    );

    let state = run_knots(&root, &db, &["state", &first_id, "planning"]);
    assert_success(&state);

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
    assert_success(&run_knots(&root, &db, &["profile", "list"]));
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
