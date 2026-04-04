use std::path::{Path, PathBuf};
use std::process::Command;

use uuid::Uuid;

use super::{
    apply_fixes, has_non_pass_checks, set_version_fix_applied_for_tests, version_fix_applied,
};
use crate::doctor::{DoctorCheck, DoctorStatus};
use crate::sync::{GitAdapter, KnotsWorktree};

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-doctor-fix-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    root
}

fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
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

fn setup_repo_with_origin() -> (PathBuf, PathBuf) {
    let root = unique_workspace();
    let origin = root.join("origin.git");
    let local = root.join("local");

    std::fs::create_dir_all(&local).expect("local directory should be creatable");
    run_git(
        &root,
        &["init", "--bare", origin.to_str().expect("utf8 origin path")],
    );
    run_git(&local, &["init"]);
    run_git(&local, &["config", "user.email", "knots@example.com"]);
    run_git(&local, &["config", "user.name", "Knots Test"]);
    std::fs::write(local.join("README.md"), "# doctor\n").expect("readme should be writable");
    run_git(&local, &["add", "README.md"]);
    run_git(&local, &["commit", "-m", "init"]);
    run_git(&local, &["branch", "-M", "main"]);
    run_git(
        &local,
        &[
            "remote",
            "add",
            "origin",
            origin.to_str().expect("utf8 origin path"),
        ],
    );
    run_git(&local, &["push", "-u", "origin", "main"]);

    (root, local)
}

fn sample_check(name: &str, status: DoctorStatus) -> DoctorCheck {
    DoctorCheck {
        name: name.to_string(),
        status,
        detail: "detail".to_string(),
    }
}

#[test]
fn has_non_pass_checks_detects_warn_or_fail() {
    let all_pass = vec![sample_check("lock_health", DoctorStatus::Pass)];
    assert!(!has_non_pass_checks(&all_pass));

    let warn = vec![sample_check("remote", DoctorStatus::Warn)];
    assert!(has_non_pass_checks(&warn));

    let fail = vec![sample_check("worktree", DoctorStatus::Fail)];
    assert!(has_non_pass_checks(&fail));
}

#[test]
fn apply_fixes_marks_version_fix_applied_for_version_check() {
    set_version_fix_applied_for_tests(false);
    let root = unique_workspace();
    let checks = vec![sample_check("version", DoctorStatus::Warn)];
    apply_fixes(&root, &checks);
    assert!(version_fix_applied());
    set_version_fix_applied_for_tests(false);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn apply_fixes_removes_lock_files() {
    let root = unique_workspace();
    let repo_lock = root.join(".knots/locks/repo.lock");
    let cache_lock = root.join(".knots/cache/cache.lock");
    std::fs::create_dir_all(repo_lock.parent().expect("repo lock parent should exist"))
        .expect("repo lock parent should be creatable");
    std::fs::create_dir_all(cache_lock.parent().expect("cache lock parent should exist"))
        .expect("cache lock parent should be creatable");
    std::fs::write(&repo_lock, "busy").expect("repo lock fixture should be writable");
    std::fs::write(&cache_lock, "busy").expect("cache lock fixture should be writable");

    let checks = vec![sample_check("lock_health", DoctorStatus::Warn)];
    apply_fixes(&root, &checks);

    assert!(!repo_lock.exists());
    assert!(!cache_lock.exists());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn apply_fixes_recreates_non_git_worktree_directory() {
    let (root, local) = setup_repo_with_origin();
    let fake_worktree = local.join(".knots").join("_worktree");
    std::fs::create_dir_all(&fake_worktree).expect("fake worktree should be creatable");
    std::fs::write(fake_worktree.join("junk.txt"), "junk")
        .expect("fake worktree fixture should be writable");

    let checks = vec![sample_check("worktree", DoctorStatus::Fail)];
    apply_fixes(&local, &checks);

    assert!(fake_worktree.join(".git").exists());
    let status = Command::new("git")
        .arg("-C")
        .arg(&fake_worktree)
        .args(["status", "--porcelain"])
        .output()
        .expect("git status should run");
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).trim().is_empty());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn apply_fixes_ignores_non_git_repo_and_unknown_checks() {
    let root = unique_workspace();
    let checks = vec![
        sample_check("worktree", DoctorStatus::Fail),
        sample_check("remote", DoctorStatus::Fail),
        sample_check("unknown_check", DoctorStatus::Warn),
        sample_check("version", DoctorStatus::Warn),
        sample_check("lock_health", DoctorStatus::Pass),
    ];

    apply_fixes(&root, &checks);
    assert!(root.exists());
    assert!(!super::run_git(&root.join("missing"), &["status"]));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn apply_fixes_cleans_legacy_and_reinstalls_hooks() {
    let (root, local) = setup_repo_with_origin();
    let hooks_dir = local.join(".git").join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();

    let legacy = "#!/usr/bin/env bash\n\
                   # knots-managed-post-commit-hook\n\
                   kno sync >/dev/null 2>&1 &\n";
    std::fs::write(hooks_dir.join("post-commit"), legacy).unwrap();

    let stale = "#!/usr/bin/env bash\n\
                  # knots-managed-post-merge-hook\n\
                  kno sync >/dev/null 2>&1 &\n";
    std::fs::write(hooks_dir.join("post-merge"), stale).unwrap();

    let checks = vec![sample_check("hooks", DoctorStatus::Warn)];
    apply_fixes(&local, &checks);

    assert!(
        !hooks_dir.join("post-commit").exists(),
        "legacy post-commit should be removed"
    );

    let pm = std::fs::read_to_string(hooks_dir.join("post-merge")).unwrap();
    assert!(
        pm.contains("kno pull"),
        "post-merge should have current template with `kno pull`"
    );
    assert!(
        !pm.contains("kno sync"),
        "post-merge should no longer contain old `kno sync`"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn apply_fixes_cleans_worktree_and_creates_remote_branch() {
    let (root, local) = setup_repo_with_origin();

    let git = GitAdapter::new();
    let worktree = KnotsWorktree::new(local.clone());
    worktree
        .ensure_exists(&git)
        .expect("worktree should be creatable for fixture setup");
    std::fs::write(worktree.path().join("dirty.txt"), "dirty")
        .expect("dirty fixture should be writable");

    let repo_lock = local.join(".knots/locks/repo.lock");
    let cache_lock = local.join(".knots/cache/cache.lock");
    std::fs::create_dir_all(repo_lock.parent().expect("repo lock parent should exist"))
        .expect("repo lock parent should be creatable");
    std::fs::create_dir_all(cache_lock.parent().expect("cache lock parent should exist"))
        .expect("cache lock parent should be creatable");
    std::fs::write(&repo_lock, "busy").expect("repo lock fixture should be writable");
    std::fs::write(&cache_lock, "busy").expect("cache lock fixture should be writable");

    let checks = vec![
        sample_check("lock_health", DoctorStatus::Warn),
        sample_check("worktree", DoctorStatus::Fail),
        sample_check("remote", DoctorStatus::Warn),
        sample_check("version", DoctorStatus::Warn),
    ];
    apply_fixes(&local, &checks);
    assert!(
        version_fix_applied(),
        "expected version fix to be applied when version check is non-pass"
    );

    let status = Command::new("git")
        .arg("-C")
        .arg(worktree.path())
        .args(["status", "--porcelain"])
        .output()
        .expect("git status should run");
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).trim().is_empty());

    let remote_branch = Command::new("git")
        .arg("-C")
        .arg(&local)
        .args(["ls-remote", "--exit-code", "--heads", "origin", "knots"])
        .output()
        .expect("git ls-remote should run");
    assert!(
        remote_branch.status.success(),
        "expected origin/knots to exist after fix, stderr: {}",
        String::from_utf8_lossy(&remote_branch.stderr)
    );
    assert!(!repo_lock.exists());
    assert!(!cache_lock.exists());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn apply_fixes_reconciles_terminal_parents() {
    let (root, local) = setup_repo_with_origin();
    let db = local.join(".knots/cache/state.sqlite");
    let app = crate::app::App::open(db.to_str().expect("db path should be utf8"), local.clone())
        .expect("app should open");
    let parent = app
        .create_knot("Parent", None, Some("implementation"), Some("default"))
        .expect("parent should be created");
    let child = app
        .create_knot("Child", None, Some("shipped"), Some("default"))
        .expect("child should be created");
    app.add_edge(&parent.id, "parent_of", &child.id)
        .expect("edge should be added");

    let checks = vec![sample_check("terminal_parents", DoctorStatus::Warn)];
    apply_fixes(&local, &checks);

    let updated = app
        .show_knot(&parent.id)
        .expect("parent should load")
        .expect("parent should exist");
    assert_eq!(updated.state, "shipped");

    let _ = std::fs::remove_dir_all(root);
}
