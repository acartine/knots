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

pub fn run_doctor(repo_root: &Path) -> Result<DoctorReport, DoctorError> {
    let checks = vec![
        check_locks(repo_root)?,
        check_worktree(repo_root),
        check_remote(repo_root)?,
        check_version(),
    ];
    Ok(DoctorReport { checks })
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

const RELEASES_API_URL: &str = "https://api.github.com/repos/acartine/knots/releases/latest";
const VERSION_CHECK_TIMEOUT_SECS: u32 = 5;

pub(crate) fn check_version() -> DoctorCheck {
    let current = env!("CARGO_PKG_VERSION");
    let tag = fetch_latest_tag(RELEASES_API_URL, VERSION_CHECK_TIMEOUT_SECS);
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

fn fetch_latest_tag(api_url: &str, timeout_secs: u32) -> Option<String> {
    let output = Command::new("curl")
        .args(["--max-time", &timeout_secs.to_string(), "-fsSL", api_url])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let body = String::from_utf8_lossy(&output.stdout);
    parse_tag(&body)
}

fn parse_tag(json: &str) -> Option<String> {
    let tag_start = json.find("\"tag_name\"")?;
    let rest = &json[tag_start..];
    let colon = rest.find(':')?;
    let after_colon = rest[colon + 1..].trim_start();
    if !after_colon.starts_with('"') {
        return None;
    }
    let value_start = 1;
    let value_end = after_colon[value_start..].find('"')?;
    Some(after_colon[value_start..value_start + value_end].to_string())
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

    use super::{build_version_check, fetch_latest_tag, is_outdated, parse_tag, strip_v_prefix};

    #[test]
    fn fetch_latest_tag_returns_none_for_unreachable_url() {
        let result = fetch_latest_tag("http://127.0.0.1:1/nonexistent", 1);
        assert_eq!(result, None);
    }

    #[test]
    fn build_version_check_warns_when_outdated() {
        let check = build_version_check("0.1.0", Some("v0.2.0".to_string()));
        assert_eq!(check.status, DoctorStatus::Warn);
        assert!(check.detail.contains("update available"));
        assert!(check.detail.contains("kno upgrade"));
    }

    #[test]
    fn build_version_check_passes_when_up_to_date() {
        let check = build_version_check("0.2.0", Some("v0.2.0".to_string()));
        assert_eq!(check.status, DoctorStatus::Pass);
        assert!(check.detail.contains("up to date"));
    }

    #[test]
    fn build_version_check_warns_on_unparseable_remote() {
        let check = build_version_check("0.2.0", Some("beta-1".to_string()));
        assert_eq!(check.status, DoctorStatus::Warn);
        assert!(check.detail.contains("unable to compare"));
    }

    #[test]
    fn build_version_check_warns_when_fetch_fails() {
        let check = build_version_check("0.2.0", None);
        assert_eq!(check.status, DoctorStatus::Warn);
        assert!(check.detail.contains("unable to check"));
    }

    #[test]
    fn parse_tag_handles_whitespace_and_nested_json() {
        let json = r#"{
            "tag_name" :  "v1.0.0" ,
            "other": "data"
        }"#;
        assert_eq!(parse_tag(json), Some("v1.0.0".to_string()));
    }

    #[test]
    fn parse_tag_returns_none_for_non_string_value() {
        assert_eq!(parse_tag(r#"{"tag_name": 123}"#), None);
    }

    #[test]
    fn parse_tag_extracts_tag_name_from_json() {
        let json = r#"{"tag_name": "v0.2.2", "name": "v0.2.2"}"#;
        assert_eq!(parse_tag(json), Some("v0.2.2".to_string()));
    }

    #[test]
    fn parse_tag_returns_none_for_missing_field() {
        assert_eq!(parse_tag(r#"{"name": "v0.2.2"}"#), None);
        assert_eq!(parse_tag("not json"), None);
        assert_eq!(parse_tag(""), None);
    }

    #[test]
    fn strip_v_prefix_removes_leading_v() {
        assert_eq!(strip_v_prefix("v1.2.3"), "1.2.3");
        assert_eq!(strip_v_prefix("1.2.3"), "1.2.3");
        assert_eq!(strip_v_prefix("v0.0.1"), "0.0.1");
    }

    #[test]
    fn is_outdated_compares_semver_parts() {
        assert_eq!(is_outdated("0.2.2", "0.2.3"), Some(true));
        assert_eq!(is_outdated("0.2.2", "0.3.0"), Some(true));
        assert_eq!(is_outdated("0.2.2", "1.0.0"), Some(true));
        assert_eq!(is_outdated("0.2.2", "0.2.2"), Some(false));
        assert_eq!(is_outdated("0.2.3", "0.2.2"), Some(false));
        assert_eq!(is_outdated("1.0.0", "0.9.9"), Some(false));
    }

    #[test]
    fn is_outdated_returns_none_for_invalid_versions() {
        assert_eq!(is_outdated("abc", "0.2.2"), None);
        assert_eq!(is_outdated("0.2.2", "abc"), None);
        assert_eq!(is_outdated("0.2", "0.2.2"), None);
        assert_eq!(is_outdated("0.2.2.1", "0.2.2"), None);
    }

    #[test]
    fn reports_failure_for_non_git_directory() {
        let root = unique_workspace();
        let report = run_doctor(&root).expect("doctor should run");
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

        let report = run_doctor(&root).expect("doctor should run");
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
}

#[cfg(test)]
#[path = "doctor_tests_ext.rs"]
mod tests_ext;
