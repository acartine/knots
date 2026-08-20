use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn unique_workspace(prefix: &str) -> knots_test_support::TestWorkspace {
    knots_test_support::workspace(prefix)
}

fn knots_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_knots"))
}

fn run_knots(repo_root: &Path, db_path: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(knots_binary())
        .arg("--repo-root")
        .arg(repo_root)
        .arg("--db")
        .arg(db_path)
        .env("HOME", home)
        .env("KNOTS_SKIP_DOCTOR_UPGRADE", "1")
        .args(args)
        .output()
        .expect("knots command should run")
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

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "expected success but failed.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn setup_repo_with_remote(root: &Path) {
    run_git(root, &["init"]);
    run_git(root, &["config", "user.email", "knots@example.com"]);
    run_git(root, &["config", "user.name", "Knots Test"]);
    std::fs::write(root.join("README.md"), "# knots\n").expect("readme should be writable");
    run_git(root, &["add", "README.md"]);
    run_git(root, &["commit", "-m", "init"]);
    run_git(root, &["branch", "-M", "main"]);
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
    run_git(root, &["push", "-u", "origin", "main"]);
}

fn first_knot_id(repo_root: &Path, db_path: &Path, home: &Path) -> String {
    let listed = run_knots(repo_root, db_path, home, &["ls", "--json"]);
    assert_success(&listed);
    let parsed: Value = serde_json::from_slice(&listed.stdout).expect("ls --json should parse");
    parsed
        .as_array()
        .and_then(|items| items.first())
        .and_then(|first| first["id"].as_str())
        .expect("ls --json should return at least one knot with an id")
        .to_string()
}

#[test]
fn claim_with_e2e_flag_emits_e2e_continuation_workflow_boundary() {
    let root_ws = unique_workspace("knots-cli-claim-e2e");
    let root = root_ws.path().to_path_buf();
    let home_ws = unique_workspace("knots-cli-claim-e2e-home");
    let home = home_ws.path().to_path_buf();
    setup_repo_with_remote(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let new = run_knots(
        &root,
        &db,
        &home,
        &["new", "E2E claim flag test", "-d", "drives e2e boundary"],
    );
    assert_success(&new);
    let id = first_knot_id(&root, &db, &home);

    let claim = run_knots(&root, &db, &home, &["claim", "--e2e", &id, "--json"]);
    assert_success(&claim);
    let parsed: Value = serde_json::from_slice(&claim.stdout).expect("claim json should parse");
    assert_eq!(parsed["e2e"], Value::Bool(true));
    assert_eq!(parsed["workflow_boundary_kind"], "e2e_continuation");
    let prompt = parsed["prompt"]
        .as_str()
        .expect("claim prompt should be a string");
    assert!(prompt.contains("kind: `e2e_continuation`"), "{prompt}");
    assert!(prompt.contains("kno claim --e2e"), "{prompt}");
    assert!(
        !prompt.contains("Complete exactly one workflow action, then stop."),
        "e2e prompt must not include the single-action stop directive: {prompt}"
    );
}

#[test]
fn claim_without_e2e_flag_keeps_single_action_workflow_boundary() {
    let root_ws = unique_workspace("knots-cli-claim-default");
    let root = root_ws.path().to_path_buf();
    let home_ws = unique_workspace("knots-cli-claim-default-home");
    let home = home_ws.path().to_path_buf();
    setup_repo_with_remote(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let new = run_knots(
        &root,
        &db,
        &home,
        &["new", "Default claim flag test", "-d", "default boundary"],
    );
    assert_success(&new);
    let id = first_knot_id(&root, &db, &home);

    let claim = run_knots(&root, &db, &home, &["claim", &id, "--json"]);
    assert_success(&claim);
    let parsed: Value = serde_json::from_slice(&claim.stdout).expect("claim json should parse");
    assert_eq!(parsed["e2e"], Value::Bool(false));
    assert_eq!(parsed["workflow_boundary_kind"], "single_action");
    let prompt = parsed["prompt"]
        .as_str()
        .expect("claim prompt should be a string");
    assert!(prompt.contains("kind: `single_action`"), "{prompt}");
    assert!(
        prompt.contains("Complete exactly one workflow action, then stop."),
        "{prompt}"
    );
    assert!(!prompt.contains("E2E continuation"), "{prompt}");
}
