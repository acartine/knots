use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::doctor::{DoctorCheck, DoctorStatus};
use crate::remote_init::init_remote_knots_branch;
use crate::sync::{GitAdapter, KnotsWorktree, SyncError};

static VERSION_FIX_APPLIED: AtomicBool = AtomicBool::new(false);

pub(crate) fn has_non_pass_checks(checks: &[DoctorCheck]) -> bool {
    checks
        .iter()
        .any(|check| check.status != DoctorStatus::Pass)
}

pub(crate) fn version_fix_applied() -> bool {
    VERSION_FIX_APPLIED.load(Ordering::Relaxed)
}

fn set_version_fix_applied(applied: bool) {
    VERSION_FIX_APPLIED.store(applied, Ordering::Relaxed);
}

#[cfg(test)]
pub(crate) fn set_version_fix_applied_for_tests(applied: bool) {
    set_version_fix_applied(applied);
}

pub(crate) fn apply_fixes(repo_root: &Path, checks: &[DoctorCheck]) {
    set_version_fix_applied(false);
    for check in checks {
        if check.status == DoctorStatus::Pass {
            continue;
        }

        match check.name.as_str() {
            "lock_health" => fix_lock_health(repo_root),
            "worktree" => fix_worktree(repo_root),
            "remote" => fix_remote(repo_root),
            "version" => fix_version(),
            "hooks" => fix_hooks(repo_root),
            _ => {}
        }
    }
}

fn fix_lock_health(repo_root: &Path) {
    let repo_lock = repo_root.join(".knots").join("locks").join("repo.lock");
    let cache_lock = repo_root.join(".knots").join("cache").join("cache.lock");
    let _ = std::fs::remove_file(repo_lock);
    let _ = std::fs::remove_file(cache_lock);
}

fn fix_worktree(repo_root: &Path) {
    if !repo_root.join(".git").exists() {
        return;
    }

    let git = GitAdapter::new();
    let worktree = KnotsWorktree::new(repo_root.to_path_buf());

    match worktree.ensure_exists(&git) {
        Ok(()) => {}
        Err(SyncError::DirtyWorktree(path)) => {
            if path.exists() && !path.join(".git").exists() {
                let _ = std::fs::remove_dir_all(&path);
                if worktree.ensure_exists(&git).is_err() {
                    return;
                }
            } else {
                return;
            }
        }
        Err(_) => return,
    }

    let worktree_path = worktree.path();
    let _ = run_git(worktree_path, &["reset", "--hard", "HEAD"]);
    let _ = run_git(worktree_path, &["clean", "-fd"]);
}

fn fix_remote(repo_root: &Path) {
    if !repo_root.join(".git").exists() {
        return;
    }
    let _ = init_remote_knots_branch(repo_root);
}

fn fix_hooks(repo_root: &Path) {
    let _ = crate::git_hooks::install_hooks(repo_root);
}

fn run_git(cwd: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(test)]
fn fix_version() {
    set_version_fix_applied(true);
}

#[cfg(not(test))]
fn fix_version() {
    if std::env::var_os("KNOTS_SKIP_DOCTOR_UPGRADE").is_some() {
        set_version_fix_applied(true);
        return;
    }

    let applied = if let Ok(exe_path) = std::env::current_exe() {
        Command::new(exe_path)
            .arg("upgrade")
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    } else {
        Command::new("kno")
            .arg("upgrade")
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    };
    set_version_fix_applied(applied);
}

#[cfg(test)]
mod tests {
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
}
