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
    let debug_binary = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/debug/knots");
    if debug_binary.exists() {
        return std::fs::canonicalize(&debug_binary).unwrap_or(debug_binary);
    }
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(debug_dir) = current_exe.parent().and_then(|deps| deps.parent()) {
            for name in ["knots", "knots.exe"] {
                let candidate = debug_dir.join(name);
                if candidate.exists() {
                    return candidate;
                }
            }
        }
    }
    let configured = PathBuf::from(env!("CARGO_BIN_EXE_knots"));
    if configured.is_absolute() && configured.exists() {
        return configured;
    }
    if configured.exists() {
        return std::fs::canonicalize(&configured).unwrap_or(configured);
    }
    let manifest_relative = Path::new(env!("CARGO_MANIFEST_DIR")).join(&configured);
    if manifest_relative.exists() {
        return std::fs::canonicalize(&manifest_relative).unwrap_or(manifest_relative);
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

fn run_repo_debug_knots(repo_root: &Path, db_path: &Path, home: &Path, args: &[&str]) -> Output {
    let binary = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/debug/knots");
    Command::new(binary)
        .arg("--repo-root")
        .arg(repo_root)
        .arg("--db")
        .arg(db_path)
        .env("HOME", home)
        .env("KNOTS_SKIP_DOCTOR_UPGRADE", "1")
        .args(args)
        .output()
        .expect("knots debug command should run")
}

fn bootstrap_builtin_workflows(repo_root: &Path, db_path: &Path, home: &Path) {
    for (knot_type, workflow_id) in [
        ("work", "work_sdlc"),
        ("gate", "gate_sdlc"),
        ("lease", "lease_sdlc"),
        ("explore", "explore_sdlc"),
    ] {
        let output = run_repo_debug_knots(
            repo_root,
            db_path,
            home,
            &["workflow", "use", workflow_id, "--type", knot_type],
        );
        assert_success(&output);
    }
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
    let stdout = String::from_utf8_lossy(&install.stdout);
    assert!(stdout.contains(".codex/skills/knots/SKILL.md"));
    assert!(stdout.contains(".codex/skills/knots-e2e/SKILL.md"));
    assert!(stdout.contains(".codex/skills/knots-create/SKILL.md"));
    assert!(home.join(".codex/skills/knots/SKILL.md").exists());
    assert!(home.join(".codex/skills/knots-create/SKILL.md").exists());

    let uninstall = run_knots(&root, &db, &home, &["skills", "uninstall", "codex"]);
    assert_success(&uninstall);
    let stdout = String::from_utf8_lossy(&uninstall.stdout);
    assert!(stdout.contains(".codex/skills/knots/SKILL.md"));
    assert!(stdout.contains(".codex/skills/knots-e2e/SKILL.md"));
    assert!(stdout.contains(".codex/skills/knots-create/SKILL.md"));
    assert!(!home.join(".codex/skills/knots/SKILL.md").exists());
    assert!(!home.join(".codex/skills/knots-create/SKILL.md").exists());
}

#[test]
fn skills_install_prefers_project_root_for_claude() {
    let root = unique_workspace("knots-cli-skills-claude");
    let home = unique_workspace("knots-cli-skills-home");
    std::fs::create_dir_all(root.join(".claude")).expect("project root should exist");
    let db = root.join(".knots/cache/state.sqlite");

    let install = run_knots(&root, &db, &home, &["skills", "install", "claude"]);
    assert_success(&install);

    assert!(root.join(".claude/skills/knots/SKILL.md").exists());
    assert!(!home.join(".claude/skills/knots/SKILL.md").exists());
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
    std::fs::create_dir_all(&project_claude).expect("project root should exist");
    let db = root.join(".knots/cache/state.sqlite");
    bootstrap_builtin_workflows(&root, &db, &home);

    let install = run_knots(&root, &db, &home, &["skills", "install", "claude"]);
    assert_success(&install);
    let project_skill = project_claude.join("skills/knots/SKILL.md");
    assert!(project_skill.exists());
    std::fs::rename(&project_skill, project_claude.join("knots.backup"))
        .expect("project skill should be movable");
    assert!(!project_skill.exists());
    assert!(project_claude.join("knots.backup").exists());

    let doctor = run_knots(&root, &db, &home, &["doctor", "--json"]);
    assert_success(&doctor);
    let report: Value = serde_json::from_slice(&doctor.stdout).expect("doctor json should parse");
    let claude = find_check(&report, "skills_claude");
    assert_eq!(claude["status"], "warn");
    let detail = claude["detail"]
        .as_str()
        .expect("detail should be a string");
    assert!(detail.contains(".claude/skills"));
    assert!(detail.contains("knots/SKILL.md"));
    assert!(detail.contains("run `kno skills install claude`"));

    let _doctor_fix = run_repo_debug_knots(&root, &db, &home, &["doctor", "--fix"]);
    assert!(project_skill.exists());

    let after = run_repo_debug_knots(&root, &db, &home, &["doctor", "--json"]);
    assert_success(&after);
    let report: Value = serde_json::from_slice(&after.stdout).expect("doctor json should parse");
    let claude = find_check(&report, "skills_claude");
    assert_eq!(claude["status"], "pass");
}

#[test]
fn skills_install_for_claude_ignores_user_level_home_root() {
    let root = unique_workspace("knots-cli-skills-claude-project-only");
    let home = unique_workspace("knots-cli-skills-home");
    std::fs::create_dir_all(home.join(".claude")).expect("user root should exist");
    let db = root.join(".knots/cache/state.sqlite");

    let install = run_knots(&root, &db, &home, &["skills", "install", "claude"]);
    assert_failure(&install);
    let stderr = String::from_utf8_lossy(&install.stderr);
    assert!(stderr.contains("Claude root not detected; create ./.claude first"));
    assert!(!home.join(".claude/skills/knots/SKILL.md").exists());
}

#[test]
fn doctor_reports_drifted_skills_and_update_reconciles_them() {
    let root = unique_workspace("knots-cli-skills-doctor-drift");
    let home = unique_workspace("knots-cli-skills-home");
    setup_repo_with_remote(&root);
    let codex_root = home.join(".codex");
    std::fs::create_dir_all(&codex_root).expect("codex root should exist");
    let db = root.join(".knots/cache/state.sqlite");
    bootstrap_builtin_workflows(&root, &db, &home);

    let install = run_knots(&root, &db, &home, &["skills", "install", "codex"]);
    assert_success(&install);
    let knots = codex_root.join("skills/knots/SKILL.md");
    std::fs::write(&knots, "stale").expect("knots skill should be writable");

    let doctor = run_knots(&root, &db, &home, &["doctor", "--json"]);
    assert_success(&doctor);
    let report: Value = serde_json::from_slice(&doctor.stdout).expect("doctor json should parse");
    let codex = find_check(&report, "skills_codex");
    assert_eq!(codex["status"], "warn");
    let detail = codex["detail"].as_str().expect("detail should be a string");
    assert!(detail.contains("drift"));
    assert!(detail.contains("run `kno skills update codex`"));
    assert!(detail.contains("knots/SKILL.md"));

    let update = run_knots(&root, &db, &home, &["skills", "update", "codex"]);
    assert_success(&update);
    assert!(std::fs::read_to_string(&knots)
        .expect("knots skill should exist")
        .contains("---"));

    let after = run_knots(&root, &db, &home, &["doctor", "--json"]);
    assert_success(&after);
    let report: Value = serde_json::from_slice(&after.stdout).expect("doctor json should parse");
    let codex = find_check(&report, "skills_codex");
    assert_eq!(codex["status"], "pass");
}

#[test]
fn doctor_fix_creates_missing_codex_root_and_installs_skills() {
    let root = unique_workspace("knots-cli-skills-doctor-fix-root");
    let home = unique_workspace("knots-cli-skills-home");
    setup_repo_with_remote(&root);
    let db = root.join(".knots/cache/state.sqlite");
    bootstrap_builtin_workflows(&root, &db, &home);

    let doctor = run_knots(&root, &db, &home, &["doctor", "--json"]);
    assert_success(&doctor);
    let report: Value = serde_json::from_slice(&doctor.stdout).expect("doctor json should parse");
    let codex = find_check(&report, "skills_codex");
    assert_eq!(codex["status"], "warn");
    let detail = codex["detail"].as_str().expect("detail should be a string");
    assert!(detail.contains(".codex/skills"));
    assert!(detail.contains("run `kno skills install codex`"));

    let _doctor_fix = run_repo_debug_knots(&root, &db, &home, &["doctor", "--fix"]);
    assert!(home.join(".codex/skills/knots/SKILL.md").exists());
    assert!(home.join(".codex/skills/knots-create/SKILL.md").exists());

    let after = run_repo_debug_knots(&root, &db, &home, &["doctor", "--json"]);
    assert_success(&after);
    let report: Value = serde_json::from_slice(&after.stdout).expect("doctor json should parse");
    let codex = find_check(&report, "skills_codex");
    assert_eq!(codex["status"], "pass");
}
