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

fn knots_binary() -> PathBuf {
    let configured = PathBuf::from(env!("CARGO_BIN_EXE_knots"));
    if configured.is_absolute() && configured.exists() {
        return configured;
    }
    if configured.exists() {
        return std::fs::canonicalize(&configured).unwrap_or(configured);
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

fn add_parent_edge(root: &Path, db: &Path, parent: &str, child: &str) {
    assert_success(&run_knots(
        root,
        db,
        &["edge", "add", parent, "parent_of", child],
    ));
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
fn next_auto_resolves_terminal_parents_recursively() {
    let root = unique_workspace("knots-cli-auto-resolve-next");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let grandparent = create_knot(&root, &db, "Grandparent", "implementation");
    let parent = create_knot(&root, &db, "Parent", "implementation");
    let child = create_knot(&root, &db, "Child", "shipment_review");
    add_parent_edge(&root, &db, &grandparent, &parent);
    add_parent_edge(&root, &db, &parent, &child);

    let output = run_knots(
        &root,
        &db,
        &["next", &child, "--expected-state", "shipment_review"],
    );
    assert_success(&output);

    assert_eq!(show_state(&root, &db, &child), "shipped");
    assert_eq!(show_state(&root, &db, &parent), "shipped");
    assert_eq!(show_state(&root, &db, &grandparent), "shipped");
}

#[test]
fn update_auto_resolves_parent_for_cascaded_descendant() {
    let root = unique_workspace("knots-cli-auto-resolve-update");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let root_parent = create_knot(&root, &db, "Root parent", "implementation");
    let other_parent = create_knot(&root, &db, "Other parent", "implementation");
    let child = create_knot(&root, &db, "Child", "implementation");
    add_parent_edge(&root, &db, &root_parent, &child);
    add_parent_edge(&root, &db, &other_parent, &child);

    let output = run_knots(
        &root,
        &db,
        &[
            "update",
            &root_parent,
            "--status",
            "abandoned",
            "--cascade-terminal-descendants",
        ],
    );
    assert_success(&output);

    assert_eq!(show_state(&root, &db, &root_parent), "abandoned");
    assert_eq!(show_state(&root, &db, &child), "abandoned");
    assert_eq!(show_state(&root, &db, &other_parent), "abandoned");
}
