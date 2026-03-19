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

fn knots_binary() -> PathBuf {
    let configured = PathBuf::from(env!("CARGO_BIN_EXE_knots"));
    if configured.is_absolute() && configured.exists() {
        return configured;
    }
    if configured.exists() {
        return std::fs::canonicalize(&configured).unwrap_or(configured);
    }
    if let Ok(current_exe) = std::env::current_exe() {
        if !configured.is_absolute() {
            for ancestor in current_exe.ancestors().skip(1) {
                let candidate = ancestor.join(&configured);
                if candidate.exists() {
                    return std::fs::canonicalize(&candidate).unwrap_or(candidate);
                }
            }
        }
        if let Some(debug_dir) = current_exe.parent().and_then(|deps| deps.parent()) {
            for name in ["knots", "knots.exe"] {
                let candidate = debug_dir.join(name);
                if candidate.exists() {
                    return candidate;
                }
            }
        }
    }
    configured
}

fn run_knots(repo_root: &Path, db_path: &Path, args: &[&str]) -> Output {
    Command::new(knots_binary())
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

fn parse_created_id(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .nth(1)
        .expect("created output should include knot id")
        .to_string()
}

fn create_knot(root: &Path, db: &Path, title: &str, state: &str) -> String {
    parse_created_id(&run_knots(
        root,
        db,
        &["new", title, "--profile", "default", "--state", state],
    ))
}

fn show_state(root: &Path, db: &Path, knot_id: &str) -> String {
    let output = run_knots(root, db, &["show", knot_id, "--json"]);
    assert_success(&output);
    let json: Value = serde_json::from_slice(&output.stdout).expect("show json should parse");
    json["state"]
        .as_str()
        .expect("show response should contain state")
        .to_string()
}

#[test]
fn doctor_warns_and_fix_resolves_terminal_parents_recursively() {
    let root = unique_workspace("knots-cli-doctor-terminal-parents");
    setup_repo_with_remote(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let grandparent = create_knot(&root, &db, "Grandparent", "implementation");
    let parent = create_knot(&root, &db, "Parent", "implementation");
    let shipped = create_knot(&root, &db, "Shipped child", "shipped");
    let deferred = create_knot(&root, &db, "Deferred child", "deferred");

    assert_success(&run_knots(
        &root,
        &db,
        &["edge", "add", &grandparent, "parent_of", &parent],
    ));
    assert_success(&run_knots(
        &root,
        &db,
        &["edge", "add", &parent, "parent_of", &shipped],
    ));
    assert_success(&run_knots(
        &root,
        &db,
        &["edge", "add", &parent, "parent_of", &deferred],
    ));

    let doctor = run_knots(&root, &db, &["doctor", "--json"]);
    assert_success(&doctor);
    let report: Value = serde_json::from_slice(&doctor.stdout).expect("doctor json should parse");
    let terminal_parents = report["checks"]
        .as_array()
        .expect("checks should be an array")
        .iter()
        .find(|check| check["name"] == "terminal_parents")
        .expect("terminal_parents check should exist");
    assert_eq!(terminal_parents["status"], "warn");
    assert!(terminal_parents["detail"]
        .as_str()
        .expect("detail should be a string")
        .contains(&parent));

    let doctor_fix = run_knots(&root, &db, &["doctor", "--fix"]);
    assert_success(&doctor_fix);
    assert_eq!(show_state(&root, &db, &parent), "shipped");
    assert_eq!(show_state(&root, &db, &grandparent), "shipped");
    assert_eq!(show_state(&root, &db, &deferred), "deferred");

    let after = run_knots(&root, &db, &["doctor", "--json"]);
    assert_success(&after);
    let report: Value = serde_json::from_slice(&after.stdout).expect("doctor json should parse");
    let terminal_parents = report["checks"]
        .as_array()
        .expect("checks should be an array")
        .iter()
        .find(|check| check["name"] == "terminal_parents")
        .expect("terminal_parents check should exist");
    assert_eq!(terminal_parents["status"], "pass");
}
