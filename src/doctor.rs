use std::error::Error;
use std::fmt;
use std::path::Path;
use std::process::Command;

use serde::Serialize;

use crate::locks::{FileLock, LockError};
use crate::state_hierarchy::find_terminal_parent_resolutions;
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

pub fn run_doctor(repo_root: &Path) -> Result<DoctorReport, DoctorError> {
    let mut checks = vec![
        check_locks(repo_root)?,
        check_worktree(repo_root),
        check_remote(repo_root)?,
        check_version(),
        crate::git_hooks::check_hooks(repo_root),
        check_stuck_leases(repo_root)?,
        check_terminal_parents(repo_root)?,
    ];
    checks.extend(crate::managed_skills::doctor_checks(repo_root));
    Ok(DoctorReport { checks })
}

pub fn run_doctor_with_fix(repo_root: &Path, fix: bool) -> Result<DoctorReport, DoctorError> {
    let report = run_doctor(repo_root)?;
    if !fix {
        return Ok(report);
    }
    crate::doctor_fix::apply_fixes(repo_root, &report.checks);
    run_doctor(repo_root)
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

const RELEASES_LATEST_URL: &str = "https://github.com/acartine/knots/releases/latest";
const VERSION_CHECK_TIMEOUT_SECS: u32 = 5;

pub(crate) fn check_version() -> DoctorCheck {
    if crate::doctor_fix::version_fix_applied() {
        return DoctorCheck {
            name: "version".to_string(),
            status: DoctorStatus::Pass,
            detail: "upgrade applied in this run; restart and rerun `kno doctor`".to_string(),
        };
    }
    let current = env!("CARGO_PKG_VERSION");
    let tag = fetch_latest_tag(RELEASES_LATEST_URL, VERSION_CHECK_TIMEOUT_SECS);
    build_version_check(current, tag)
}

fn build_version_check(current: &str, tag: Option<String>) -> DoctorCheck {
    match tag {
        Some(tag) => {
            let latest = strip_v_prefix(&tag);
            match is_outdated(current, latest) {
                Some(true) => DoctorCheck {
                    name: "version".to_string(),
                    status: DoctorStatus::Warn,
                    detail: format!(
                        "update available: v{current} -> v{latest} \
                         (run `kno upgrade`)"
                    ),
                },
                Some(false) => DoctorCheck {
                    name: "version".to_string(),
                    status: DoctorStatus::Pass,
                    detail: format!("v{current} is up to date"),
                },
                None => DoctorCheck {
                    name: "version".to_string(),
                    status: DoctorStatus::Warn,
                    detail: format!("unable to compare v{current} with remote {tag}"),
                },
            }
        }
        None => DoctorCheck {
            name: "version".to_string(),
            status: DoctorStatus::Warn,
            detail: format!("v{current} (unable to check for updates)"),
        },
    }
}

fn fetch_latest_tag(url: &str, timeout_secs: u32) -> Option<String> {
    // Use HEAD + redirect to avoid GitHub API rate limits.
    let output = Command::new("curl")
        .args(["--max-time", &timeout_secs.to_string(), "-fsS", "-I", url])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let headers = String::from_utf8_lossy(&output.stdout);
    parse_location_tag(&headers)
}

fn parse_location_tag(headers: &str) -> Option<String> {
    for line in headers.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("location:") {
            let url = line.split_once(':')?.1.trim();
            let tag = url.rsplit('/').next()?;
            if !tag.is_empty() {
                return Some(tag.to_string());
            }
        }
    }
    None
}

fn strip_v_prefix(tag: &str) -> &str {
    tag.strip_prefix('v').unwrap_or(tag)
}

fn is_outdated(current: &str, latest: &str) -> Option<bool> {
    let cur: Vec<u64> = current
        .split('.')
        .map(|s| s.parse().ok())
        .collect::<Option<Vec<_>>>()?;
    let lat: Vec<u64> = latest
        .split('.')
        .map(|s| s.parse().ok())
        .collect::<Option<Vec<_>>>()?;
    if cur.len() != 3 || lat.len() != 3 {
        return None;
    }
    Some(cur < lat)
}

fn check_stuck_leases(repo_root: &Path) -> Result<DoctorCheck, DoctorError> {
    let db_path = repo_root.join(".knots").join("cache").join("state.sqlite");
    if !db_path.exists() {
        return Ok(DoctorCheck {
            name: "stuck_leases".to_string(),
            status: DoctorStatus::Pass,
            detail: "no cache database found".to_string(),
        });
    }
    let conn = crate::db::open_connection(db_path.to_str().unwrap_or(".knots/cache/state.sqlite"))
        .map_err(|e| DoctorError::Io(std::io::Error::other(e.to_string())))?;

    let count = crate::db::count_active_leases(&conn)
        .map_err(|e| DoctorError::Io(std::io::Error::other(e.to_string())))?;

    if count > 0 {
        Ok(DoctorCheck {
            name: "stuck_leases".to_string(),
            status: DoctorStatus::Warn,
            detail: format!("{} active lease(s) may be stuck", count),
        })
    } else {
        Ok(DoctorCheck {
            name: "stuck_leases".to_string(),
            status: DoctorStatus::Pass,
            detail: "no stuck leases".to_string(),
        })
    }
}

fn check_terminal_parents(repo_root: &Path) -> Result<DoctorCheck, DoctorError> {
    let db_path = repo_root.join(".knots").join("cache").join("state.sqlite");
    if !db_path.exists() {
        return Ok(DoctorCheck {
            name: "terminal_parents".to_string(),
            status: DoctorStatus::Pass,
            detail: "no cache database found".to_string(),
        });
    }

    let conn = crate::db::open_connection(db_path.to_str().unwrap_or(".knots/cache/state.sqlite"))
        .map_err(|err| DoctorError::Io(std::io::Error::other(err.to_string())))?;
    let resolutions = find_terminal_parent_resolutions(&conn)
        .map_err(|err| DoctorError::Io(std::io::Error::other(err.to_string())))?;

    if resolutions.is_empty() {
        return Ok(DoctorCheck {
            name: "terminal_parents".to_string(),
            status: DoctorStatus::Pass,
            detail: "no parent knots require terminal reconciliation".to_string(),
        });
    }

    let summary = resolutions
        .iter()
        .map(|resolution| format!("{} -> {}", resolution.parent.id, resolution.target_state))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(DoctorCheck {
        name: "terminal_parents".to_string(),
        status: DoctorStatus::Warn,
        detail: format!(
            "{} parent knot(s) have only terminal children: {} (run `kno doctor --fix`)",
            resolutions.len(),
            summary
        ),
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
#[path = "doctor_tests_core.rs"]
mod tests;

#[cfg(test)]
#[path = "doctor_tests_ext.rs"]
mod tests_ext;
