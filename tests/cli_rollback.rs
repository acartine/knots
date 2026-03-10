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

fn assert_failure(output: &Output) {
    assert!(
        !output.status.success(),
        "expected failure but succeeded.\nstdout:\n{}\nstderr:\n{}",
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

#[test]
fn rollback_alias_rewinds_review_state_to_prior_ready_state() {
    let root = unique_workspace("knots-cli-rollback-review");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Rollback review",
            "--profile",
            "autopilot",
            "--state",
            "ready_for_implementation",
        ],
    );
    assert_success(&created);
    let knot_id = parse_created_id(&created);

    assert_success(&run_knots(
        &root,
        &db,
        &["state", &knot_id, "implementation"],
    ));
    assert_success(&run_knots(
        &root,
        &db,
        &["next", &knot_id, "implementation"],
    ));
    assert_success(&run_knots(
        &root,
        &db,
        &["next", &knot_id, "ready_for_implementation_review"],
    ));

    let rollback = run_knots(&root, &db, &["rb", &knot_id]);
    assert_success(&rollback);
    let stdout = String::from_utf8_lossy(&rollback.stdout);
    assert!(stdout.contains("rolled back"));
    assert!(stdout.contains("ready_for_implementation"));

    let show = run_knots(&root, &db, &["show", &knot_id, "--json"]);
    assert_success(&show);
    let knot: Value = serde_json::from_slice(&show.stdout).expect("show should emit json");
    assert_eq!(knot["state"], "ready_for_implementation");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn rollback_dry_run_previews_without_mutating_and_rejects_queue_states() {
    let root = unique_workspace("knots-cli-rollback-dry-run");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Rollback dry run",
            "--profile",
            "autopilot",
            "--state",
            "ready_for_implementation",
        ],
    );
    assert_success(&created);
    let knot_id = parse_created_id(&created);

    let queue_reject = run_knots(&root, &db, &["rollback", &knot_id]);
    assert_failure(&queue_reject);
    assert!(String::from_utf8_lossy(&queue_reject.stderr).contains("queue state"));

    assert_success(&run_knots(
        &root,
        &db,
        &["state", &knot_id, "implementation"],
    ));

    let dry_run = run_knots(&root, &db, &["rollback", &knot_id, "--dry-run"]);
    assert_success(&dry_run);
    let stdout = String::from_utf8_lossy(&dry_run.stdout);
    assert!(stdout.contains("would roll back"));
    assert!(stdout.contains("preceding ready state"));

    let show = run_knots(&root, &db, &["show", &knot_id, "--json"]);
    assert_success(&show);
    let knot: Value = serde_json::from_slice(&show.stdout).expect("show should emit json");
    assert_eq!(knot["state"], "implementation");

    let _ = std::fs::remove_dir_all(root);
}
