use std::error::Error;
use std::fmt;
use std::path::Path;
use std::process::Command;

use serde::Serialize;

use crate::locks::{FileLock, LockError};
use crate::sync::{GitAdapter, KnotsWorktree, SyncError};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DoctorStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DoctorCheck {
    pub name: String,
    pub status: DoctorStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DoctorReport {
    pub checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    pub fn failure_count(&self) -> usize {
        self.checks
            .iter()
            .filter(|check| check.status == DoctorStatus::Fail)
            .count()
    }
}

#[derive(Debug)]
pub enum DoctorError {
    Io(std::io::Error),
    Lock(LockError),
}

impl fmt::Display for DoctorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DoctorError::Io(err) => write!(f, "I/O error: {}", err),
            DoctorError::Lock(err) => write!(f, "lock error: {}", err),
        }
    }
}

impl Error for DoctorError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            DoctorError::Io(err) => Some(err),
            DoctorError::Lock(err) => Some(err),
        }
    }
}

impl From<std::io::Error> for DoctorError {
    fn from(value: std::io::Error) -> Self {
        DoctorError::Io(value)
    }
}

impl From<LockError> for DoctorError {
    fn from(value: LockError) -> Self {
        DoctorError::Lock(value)
    }
}

pub fn run_doctor(repo_root: &Path, fix: bool) -> Result<DoctorReport, DoctorError> {
    let checks = vec![
        check_workflows(repo_root, fix)?,
        check_locks(repo_root)?,
        check_worktree(repo_root),
        check_remote(repo_root)?,
    ];
    Ok(DoctorReport { checks })
}

fn check_workflows(repo_root: &Path, fix: bool) -> Result<DoctorCheck, DoctorError> {
    let path = repo_root.join(".knots/workflows.toml");
    if path.exists() {
        return Ok(DoctorCheck {
            name: "workflows".to_string(),
            status: DoctorStatus::Pass,
            detail: "workflows.toml exists".to_string(),
        });
    }

    if fix {
        if let Err(err) = crate::init::ensure_workflows_file(repo_root) {
            return Ok(DoctorCheck {
                name: "workflows".to_string(),
                status: DoctorStatus::Fail,
                detail: format!("failed to create workflows.toml: {}", err),
            });
        }
        return Ok(DoctorCheck {
            name: "workflows".to_string(),
            status: DoctorStatus::Pass,
            detail: "workflows.toml created with defaults".to_string(),
        });
    }

    Ok(DoctorCheck {
        name: "workflows".to_string(),
        status: DoctorStatus::Fail,
        detail: "workflows.toml missing (run `kno doctor --fix`)".to_string(),
    })
}

fn check_locks(repo_root: &Path) -> Result<DoctorCheck, DoctorError> {
    let repo_lock_path = repo_root.join(".knots").join("locks").join("repo.lock");
    let cache_lock_path = repo_root.join(".knots").join("cache").join("cache.lock");

    let repo_guard = FileLock::try_acquire(&repo_lock_path)?;
    let cache_guard = FileLock::try_acquire(&cache_lock_path)?;

    let status = if repo_guard.is_some() && cache_guard.is_some() {
        DoctorStatus::Pass
    } else {
        DoctorStatus::Warn
    };
    let detail = if status == DoctorStatus::Pass {
        "repo/cache locks are acquirable".to_string()
    } else {
        "one or more locks are currently busy".to_string()
    };

    drop(repo_guard);
    drop(cache_guard);

    Ok(DoctorCheck {
        name: "lock_health".to_string(),
        status,
        detail,
    })
}

fn check_worktree(repo_root: &Path) -> DoctorCheck {
    if !repo_root.join(".git").exists() {
        return DoctorCheck {
            name: "worktree".to_string(),
            status: DoctorStatus::Fail,
            detail: "not a git repository".to_string(),
        };
    }

    let git = GitAdapter::new();
    let worktree = KnotsWorktree::new(repo_root.to_path_buf());
    let result = worktree
        .ensure_exists(&git)
        .and_then(|()| worktree.ensure_clean(&git));

    match result {
        Ok(()) => DoctorCheck {
            name: "worktree".to_string(),
            status: DoctorStatus::Pass,
            detail: "knots worktree is clean".to_string(),
        },
        Err(SyncError::DirtyWorktree(path)) => DoctorCheck {
            name: "worktree".to_string(),
            status: DoctorStatus::Fail,
            detail: format!("worktree is dirty: {}", path.display()),
        },
        Err(err) => DoctorCheck {
            name: "worktree".to_string(),
            status: DoctorStatus::Fail,
            detail: format!("worktree check failed: {}", err),
        },
    }
}

fn check_remote(repo_root: &Path) -> Result<DoctorCheck, DoctorError> {
    if !repo_root.join(".git").exists() {
        return Ok(DoctorCheck {
            name: "remote".to_string(),
            status: DoctorStatus::Fail,
            detail: "not a git repository".to_string(),
        });
    }

    let remote_url = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["remote", "get-url", "origin"])
        .output()?;

    if !remote_url.status.success() {
        return Ok(DoctorCheck {
            name: "remote".to_string(),
            status: DoctorStatus::Fail,
            detail: "remote 'origin' is not configured".to_string(),
        });
    }

    let ls_remote = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["ls-remote", "--heads", "origin"])
        .output()?;

    if !ls_remote.status.success() {
        return Ok(DoctorCheck {
            name: "remote".to_string(),
            status: DoctorStatus::Fail,
            detail: format!(
                "origin is not reachable: {}",
                String::from_utf8_lossy(&ls_remote.stderr).trim()
            ),
        });
    }

    let knots_exists = String::from_utf8_lossy(&ls_remote.stdout)
        .lines()
        .any(|line| line.contains("refs/heads/knots"));

    let (status, detail) = if knots_exists {
        (
            DoctorStatus::Pass,
            "origin reachable; knots branch exists".to_string(),
        )
    } else {
        (
            DoctorStatus::Warn,
            "origin reachable; knots branch missing (run `kno init`)".to_string(),
        )
    };

    Ok(DoctorCheck {
        name: "remote".to_string(),
        status,
        detail,
    })
}

#[cfg(test)]
pub fn wait_for_lock_release(
    lock_path: &Path,
    timeout: std::time::Duration,
) -> Result<bool, DoctorError> {
    let start = std::time::Instant::now();
    while start.elapsed() <= timeout {
        let acquired = FileLock::try_acquire(lock_path)?;
        if let Some(guard) = acquired {
            drop(guard);
            return Ok(true);
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::Duration;

    use uuid::Uuid;

    use super::{run_doctor, wait_for_lock_release, DoctorStatus};
    use crate::locks::FileLock;

    fn unique_workspace() -> PathBuf {
        let root = std::env::temp_dir().join(format!("knots-doctor-test-{}", Uuid::now_v7()));
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

    #[test]
    fn reports_failure_for_non_git_directory() {
        let root = unique_workspace();
        let report = run_doctor(&root, false).expect("doctor should run");
        assert!(report.failure_count() >= 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reports_busy_lock_as_warning() {
        let root = unique_workspace();
        run_git(&root, &["init"]);
        run_git(&root, &["config", "user.email", "knots@example.com"]);
        run_git(&root, &["config", "user.name", "Knots Test"]);
        std::fs::write(root.join("README.md"), "# test\n").expect("readme should write");
        run_git(&root, &["add", "README.md"]);
        run_git(&root, &["commit", "-m", "init"]);

        let lock_path = root.join(".knots").join("locks").join("repo.lock");
        let _guard = FileLock::acquire(&lock_path, Duration::from_millis(100))
            .expect("lock acquisition should succeed");

        let report = run_doctor(&root, false).expect("doctor should run");
        let lock_check = report
            .checks
            .iter()
            .find(|check| check.name == "lock_health")
            .expect("lock health check should exist");
        assert_eq!(lock_check.status, DoctorStatus::Warn);

        assert!(
            !wait_for_lock_release(&lock_path, Duration::from_millis(10))
                .expect("lock wait should succeed")
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn fix_creates_missing_workflows_file() {
        let root = unique_workspace();
        let workflow_path = root.join(".knots/workflows.toml");

        let report = run_doctor(&root, false).expect("doctor should run");
        let wf_check = report
            .checks
            .iter()
            .find(|c| c.name == "workflows")
            .expect("workflows check should exist");
        assert_eq!(wf_check.status, DoctorStatus::Fail);
        assert!(!workflow_path.exists());

        let fixed = run_doctor(&root, true).expect("doctor --fix should run");
        let wf_fixed = fixed
            .checks
            .iter()
            .find(|c| c.name == "workflows")
            .expect("workflows check should exist");
        assert_eq!(wf_fixed.status, DoctorStatus::Pass);
        assert!(workflow_path.exists());

        let _ = std::fs::remove_dir_all(root);
    }
}
