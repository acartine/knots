use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use uuid::Uuid;

fn unique_workspace(prefix: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&path).expect("workspace should be creatable");
    path
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

fn assert_failure(output: &Output) {
    assert!(
        !output.status.success(),
        "expected failure but command succeeded.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn find_check<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["checks"]
        .as_array()
        .expect("checks should be an array")
        .iter()
        .find(|check| check["name"] == name)
        .expect("expected check to exist")
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

#[test]
fn skills_install_and_uninstall_round_trip_for_codex() {
    let root = unique_workspace("knots-cli-skills-codex");
    let home = unique_workspace("knots-cli-skills-home");
    std::fs::create_dir_all(home.join(".codex")).expect("codex root should exist");
    let db = root.join(".knots/cache/state.sqlite");

    let install = run_knots(&root, &db, &home, &["skills", "install", "codex"]);
    assert_success(&install);
    assert!(home.join(".codex/skills/planning/SKILL.md").exists());

    let uninstall = run_knots(&root, &db, &home, &["skills", "uninstall", "codex"]);
    assert_success(&uninstall);
    assert!(!home.join(".codex/skills/planning/SKILL.md").exists());
}

#[test]
fn skills_install_prefers_project_root_for_claude() {
    let root = unique_workspace("knots-cli-skills-claude");
    let home = unique_workspace("knots-cli-skills-home");
    std::fs::create_dir_all(root.join(".claude")).expect("project root should exist");
    std::fs::create_dir_all(home.join(".claude")).expect("user root should exist");
    let db = root.join(".knots/cache/state.sqlite");

    let install = run_knots(&root, &db, &home, &["skills", "install", "claude"]);
    assert_success(&install);

    assert!(root.join(".claude/skills/planning/SKILL.md").exists());
    assert!(!home.join(".claude/skills/planning/SKILL.md").exists());
}

#[test]
fn skills_update_fails_non_interactively_when_install_is_required() {
    let root = unique_workspace("knots-cli-skills-update");
    let home = unique_workspace("knots-cli-skills-home");
    std::fs::create_dir_all(home.join(".config/opencode")).expect("opencode root should exist");
    let db = root.join(".knots/cache/state.sqlite");

    let update = run_knots(&root, &db, &home, &["skills", "update", "opencode"]);
    assert_failure(&update);
    let stderr = String::from_utf8_lossy(&update.stderr);
    assert!(stderr.contains("run `kno skills install opencode`"));
}

#[test]
fn doctor_reports_missing_skills_and_fix_installs_for_preferred_root() {
    let root = unique_workspace("knots-cli-skills-doctor");
    let home = unique_workspace("knots-cli-skills-home");
    setup_repo_with_remote(&root);
    let project_claude = root.join(".claude");
    let user_claude = home.join(".claude");
    std::fs::create_dir_all(&project_claude).expect("project root should exist");
    std::fs::create_dir_all(&user_claude).expect("user root should exist");
    let db = root.join(".knots/cache/state.sqlite");

    let user_install = run_knots(&root, &db, &home, &["skills", "install", "claude"]);
    assert_success(&user_install);
    let planning_skill = user_claude.join("skills/planning/SKILL.md");
    let project_planning_skill = project_claude.join("skills/planning/SKILL.md");
    assert!(project_planning_skill.exists());
    std::fs::rename(
        &project_planning_skill,
        project_claude.join("planning.backup"),
    )
    .expect("project skill should be movable");
    assert!(!project_planning_skill.exists());
    assert!(!planning_skill.exists());
    assert!(project_claude.join("planning.backup").exists());

    let doctor = run_knots(&root, &db, &home, &["doctor", "--json"]);
    assert_success(&doctor);
    let report: Value = serde_json::from_slice(&doctor.stdout).expect("doctor json should parse");
    let claude = find_check(&report, "skills_claude");
    assert_eq!(claude["status"], "warn");
    let detail = claude["detail"]
        .as_str()
        .expect("detail should be a string");
    assert!(detail.contains(".claude/skills"));
    assert!(detail.contains("run `kno skills install claude`"));

    let doctor_fix = run_knots(&root, &db, &home, &["doctor", "--fix"]);
    assert_success(&doctor_fix);
    assert!(project_planning_skill.exists());
    assert!(!planning_skill.exists());

    let after = run_knots(&root, &db, &home, &["doctor", "--json"]);
    assert_success(&after);
    let report: Value = serde_json::from_slice(&after.stdout).expect("doctor json should parse");
    let claude = find_check(&report, "skills_claude");
    assert_eq!(claude["status"], "pass");
}
