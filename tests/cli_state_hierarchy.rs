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

fn configure_coverage_env(command: &mut Command) {
    let _ = command;
}

fn run_knots(repo_root: &Path, db_path: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(knots_binary());
    command
        .arg("--repo-root")
        .arg(repo_root)
        .arg("--db")
        .arg(db_path)
        .env("KNOTS_SKIP_DOCTOR_UPGRADE", "1")
        .args(args);
    configure_coverage_env(&mut command);
    command.output().expect("knots command should run")
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
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .nth(1)
        .expect("created output should include knot id")
        .to_string()
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

#[test]
fn state_rejection_reports_code_and_blocking_child() {
    let root = unique_workspace("knots-cli-hierarchy-state");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let parent = parse_created_id(&run_knots(
        &root,
        &db,
        &[
            "new",
            "Parent",
            "--profile",
            "default",
            "--state",
            "planning",
        ],
    ));
    let child = parse_created_id(&run_knots(
        &root,
        &db,
        &[
            "new",
            "Child",
            "--profile",
            "default",
            "--state",
            "planning",
        ],
    ));
    assert_success(&run_knots(
        &root,
        &db,
        &["edge", "add", &parent, "parent_of", &child],
    ));

    let output = run_knots(&root, &db, &["state", &parent, "ready_for_plan_review"]);
    assert_failure(&output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("hierarchy_progress_blocked"));
    assert!(stderr.contains("direct child knots are behind"));
    assert!(stderr.contains(&child));
}

#[test]
fn update_status_rejection_reports_code_and_blocking_child() {
    let root = unique_workspace("knots-cli-hierarchy-update");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let parent = parse_created_id(&run_knots(
        &root,
        &db,
        &[
            "new",
            "Parent",
            "--profile",
            "default",
            "--state",
            "planning",
        ],
    ));
    let child = parse_created_id(&run_knots(
        &root,
        &db,
        &[
            "new",
            "Child",
            "--profile",
            "default",
            "--state",
            "planning",
        ],
    ));
    assert_success(&run_knots(
        &root,
        &db,
        &["edge", "add", &parent, "parent_of", &child],
    ));

    let output = run_knots(
        &root,
        &db,
        &["update", &parent, "--status", "ready_for_plan_review"],
    );
    assert_failure(&output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("hierarchy_progress_blocked"));
    assert!(stderr.contains("ready_for_plan_review"));
    assert!(stderr.contains(&child));
}

#[test]
fn next_rejection_reports_code_and_blocking_child() {
    let root = unique_workspace("knots-cli-hierarchy-next");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let parent = parse_created_id(&run_knots(
        &root,
        &db,
        &[
            "new",
            "Parent",
            "--profile",
            "default",
            "--state",
            "planning",
        ],
    ));
    let child = parse_created_id(&run_knots(
        &root,
        &db,
        &[
            "new",
            "Child",
            "--profile",
            "default",
            "--state",
            "planning",
        ],
    ));
    assert_success(&run_knots(
        &root,
        &db,
        &["edge", "add", &parent, "parent_of", &child],
    ));

    let output = run_knots(
        &root,
        &db,
        &["next", &parent, "--expected-state", "planning"],
    );
    assert_failure(&output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("hierarchy_progress_blocked"));
    assert!(stderr.contains(&child));
}

#[test]
fn terminal_state_requires_flag_and_state_flag_cascades_descendants() {
    let root = unique_workspace("knots-cli-hierarchy-terminal");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let parent = parse_created_id(&run_knots(
        &root,
        &db,
        &[
            "new",
            "Parent",
            "--profile",
            "default",
            "--state",
            "implementation",
        ],
    ));
    let child = parse_created_id(&run_knots(
        &root,
        &db,
        &[
            "new",
            "Child",
            "--profile",
            "default",
            "--state",
            "planning",
        ],
    ));
    let grandchild = parse_created_id(&run_knots(
        &root,
        &db,
        &[
            "new",
            "Grandchild",
            "--profile",
            "default",
            "--state",
            "idea",
        ],
    ));
    assert_success(&run_knots(
        &root,
        &db,
        &["edge", "add", &parent, "parent_of", &child],
    ));
    assert_success(&run_knots(
        &root,
        &db,
        &["edge", "add", &child, "parent_of", &grandchild],
    ));

    let rejected = run_knots(&root, &db, &["state", &parent, "abandoned"]);
    assert_failure(&rejected);
    let stderr = String::from_utf8_lossy(&rejected.stderr);
    assert!(stderr.contains("terminal_cascade_approval_required"));
    assert!(stderr.contains(&child));
    assert!(stderr.contains(&grandchild));
    assert!(stderr.contains("--cascade-terminal-descendants"));

    let approved = run_knots(
        &root,
        &db,
        &[
            "state",
            &parent,
            "abandoned",
            "--cascade-terminal-descendants",
        ],
    );
    assert_success(&approved);
    assert_eq!(show_state(&root, &db, &parent), "abandoned");
    assert_eq!(show_state(&root, &db, &child), "abandoned");
    assert_eq!(show_state(&root, &db, &grandchild), "abandoned");
}

#[test]
fn next_terminal_requires_flag_and_cascades_descendants() {
    let root = unique_workspace("knots-cli-hierarchy-next-terminal");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let parent = parse_created_id(&run_knots(
        &root,
        &db,
        &[
            "new",
            "Parent",
            "--profile",
            "default",
            "--state",
            "shipment_review",
        ],
    ));
    let child = parse_created_id(&run_knots(
        &root,
        &db,
        &[
            "new",
            "Child",
            "--profile",
            "default",
            "--state",
            "implementation",
        ],
    ));
    assert_success(&run_knots(
        &root,
        &db,
        &["edge", "add", &parent, "parent_of", &child],
    ));

    let rejected = run_knots(
        &root,
        &db,
        &["next", &parent, "--expected-state", "shipment_review"],
    );
    assert_failure(&rejected);
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("terminal_cascade_approval_required")
    );

    let approved = run_knots(
        &root,
        &db,
        &[
            "next",
            &parent,
            "--expected-state",
            "shipment_review",
            "--cascade-terminal-descendants",
        ],
    );
    assert_success(&approved);
    assert_eq!(show_state(&root, &db, &parent), "shipped");
    assert_eq!(show_state(&root, &db, &child), "shipped");
}

#[test]
fn deferred_child_blocks_state_and_reports_provenance() {
    let root = unique_workspace("knots-cli-hierarchy-deferred");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let parent = create_knot(&root, &db, "Parent", "implementation");
    let child = create_knot(&root, &db, "Child", "implementation");
    add_parent_edge(&root, &db, &parent, &child);
    assert_success(&run_knots(&root, &db, &["state", &child, "deferred"]));

    let output = run_knots(
        &root,
        &db,
        &[
            "state",
            &parent,
            "ready_for_implementation_review",
            "--force",
        ],
    );
    assert_failure(&output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("hierarchy_progress_blocked"));
    assert!(stderr.contains("deferred from implementation"));
    assert!(stderr.contains(&child));
}

#[test]
fn update_terminal_cascade_flag_cascades_descendants() {
    let root = unique_workspace("knots-cli-hierarchy-update-cascade");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let parent = create_knot(&root, &db, "Parent", "implementation");
    let child = create_knot(&root, &db, "Child", "planning");
    add_parent_edge(&root, &db, &parent, &child);

    let rejected = run_knots(&root, &db, &["update", &parent, "--status", "abandoned"]);
    assert_failure(&rejected);
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("terminal_cascade_approval_required")
    );

    let approved = run_knots(
        &root,
        &db,
        &[
            "update",
            &parent,
            "--status",
            "abandoned",
            "--cascade-terminal-descendants",
        ],
    );
    assert_success(&approved);
    assert_eq!(show_state(&root, &db, &parent), "abandoned");
    assert_eq!(show_state(&root, &db, &child), "abandoned");
}
