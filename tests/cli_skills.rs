use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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
