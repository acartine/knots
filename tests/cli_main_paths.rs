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

fn run_knots(repo_root: &Path, db_path: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_knots"))
        .arg("--repo-root")
        .arg(repo_root)
        .arg("--db")
        .arg(db_path)
        .env("KNOTS_SKIP_DOCTOR_UPGRADE", "1")
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
fn toplevel_help_uses_custom_help_path() {
    let root = unique_workspace("knots-main-help");
    setup_repo(&root);

    let output = Command::new(env!("CARGO_BIN_EXE_knots"))
        .current_dir(&root)
        .env("KNOTS_SKIP_DOCTOR_UPGRADE", "1")
        .output()
        .expect("knots command should run");
    assert_success(&output);

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Common Commands:"), "stdout: {stdout}");
    assert!(stdout.contains("Other Commands:"), "stdout: {stdout}");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn fsck_non_json_failure_prints_issue_rows() {
    let root = unique_workspace("knots-main-fsck-issues");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Broken fsck input",
            "--profile",
            "autopilot",
            "--state",
            "idea",
        ],
    );
    assert_success(&created);

    let bad_file = root.join(".knots/index/bad-event.json");
    std::fs::create_dir_all(
        bad_file
            .parent()
            .expect("bad fsck file should always have a parent"),
    )
    .expect("index directory should be creatable");
    std::fs::write(&bad_file, "{ this is not valid json").expect("invalid fsck file should write");

    let fsck = run_knots(&root, &db, &["fsck"]);
    assert_failure(&fsck);
    let stdout = String::from_utf8_lossy(&fsck.stdout);
    let stderr = String::from_utf8_lossy(&fsck.stderr);
    assert!(stdout.contains("issues="), "stdout: {stdout}");
    assert!(stdout.contains("invalid JSON payload"), "stdout: {stdout}");
    assert!(stderr.contains("fsck found"), "stderr: {stderr}");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn ready_claim_peek_skill_terminal_and_rehydrate_missing_paths() {
    let root = unique_workspace("knots-main-branches");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let ready_knot = run_knots(
        &root,
        &db,
        &[
            "new",
            "Peek candidate",
            "--profile",
            "autopilot",
            "--state",
            "ready_for_implementation",
        ],
    );
    assert_success(&ready_knot);
    let ready_id = parse_created_id(&ready_knot);

    let ready = run_knots(&root, &db, &["ready"]);
    assert_success(&ready);
    assert!(
        String::from_utf8_lossy(&ready.stdout).contains("Peek candidate"),
        "ready should include knot title"
    );

    let peek = run_knots(&root, &db, &["claim", &ready_id, "--peek"]);
    assert_success(&peek);

    let shown = run_knots(&root, &db, &["show", &ready_id, "--json"]);
    assert_success(&shown);
    let shown_json: Value = serde_json::from_slice(&shown.stdout).expect("show should return json");
    assert_eq!(shown_json["state"], "ready_for_implementation");

    let shipped = run_knots(
        &root,
        &db,
        &[
            "new",
            "Terminal skill",
            "--profile",
            "autopilot",
            "--state",
            "shipped",
        ],
    );
    assert_success(&shipped);
    let shipped_id = parse_created_id(&shipped);

    let skill_terminal = run_knots(&root, &db, &["skill", &shipped_id]);
    assert_failure(&skill_terminal);
    assert!(
        String::from_utf8_lossy(&skill_terminal.stderr).contains("no next state"),
        "terminal skill should report no next state"
    );

    let missing_rehydrate = run_knots(&root, &db, &["rehydrate", "missing-id"]);
    assert_failure(&missing_rehydrate);
    assert!(
        String::from_utf8_lossy(&missing_rehydrate.stderr).contains("not found"),
        "rehydrate missing should return not found"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn hooks_status_command_dispatches_through_main() {
    let root = unique_workspace("knots-main-hooks-dispatch");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let output = run_knots(&root, &db, &["hooks", "status"]);
    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("missing"), "stdout: {stdout}");

    let _ = std::fs::remove_dir_all(root);
}
