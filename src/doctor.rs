use std::error::Error;
use std::fmt;
use std::path::Path;
use std::process::Command;

use serde::Serialize;

use crate::locks::{FileLock, LockError};
use crate::progress::{emit_progress, ProgressKind, ProgressReporter};
use crate::project::{DistributionMode, StorePaths};
use crate::release_version::{fetch_latest_tag, is_outdated, strip_v_prefix, RELEASES_LATEST_URL};
use crate::state_hierarchy::find_terminal_parent_resolutions;
use crate::sync::{GitAdapter, KnotsWorktree, SyncError};
use crate::sync_ref::SyncRefConfig;

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
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub data: Option<serde_json::Value>,
}

impl DoctorCheck {
    pub fn simple(
        name: impl Into<String>,
        status: DoctorStatus,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            status,
            detail: detail.into(),
            data: None,
        }
    }
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

#[cfg(test)]
pub fn run_doctor(repo_root: &Path) -> Result<DoctorReport, DoctorError> {
    run_doctor_at(repo_root, &repo_root.join(".knots"), DistributionMode::Git)
}

pub fn run_doctor_at(
    repo_root: &Path,
    store_root: &Path,
    distribution: DistributionMode,
) -> Result<DoctorReport, DoctorError> {
    let store_paths = StorePaths {
        root: store_root.to_path_buf(),
    };
    let mut checks = vec![
        check_locks(&store_paths)?,
        check_worktree(repo_root, &store_paths, distribution),
        check_remote(repo_root, distribution)?,
        crate::doctor_sync_ref::check_legacy_knots_head(repo_root, distribution),
        crate::doctor_gitignore::check_gitignore(repo_root, distribution),
        check_version(),
        check_hooks(repo_root, distribution),
        crate::doctor_workflows::check_registered_workflows(repo_root),
        check_schema_version(&store_paths)?,
        check_stuck_leases(&store_paths)?,
        check_terminal_parents(repo_root, &store_paths)?,
        crate::doctor_cold_tier::check_cold_tier_imbalance_at(&store_paths)?,
        crate::doctor_workflow_parity::check_workflow_id_parity_at(&store_paths)?,
        crate::doctor_knot_type_backfill::check_knot_type_backfill_at(&store_paths)?,
        crate::doctor_nested_cache::check_nested_caches_at(&store_paths)?,
    ];
    checks.extend(crate::managed_skills::doctor_checks(repo_root));
    Ok(DoctorReport { checks })
}

#[cfg(test)]
#[allow(dead_code)]
pub fn run_doctor_with_fix(repo_root: &Path, fix: bool) -> Result<DoctorReport, DoctorError> {
    run_doctor_with_fix_at(
        repo_root,
        &repo_root.join(".knots"),
        DistributionMode::Git,
        fix,
    )
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn run_doctor_with_fix_at(
    repo_root: &Path,
    store_root: &Path,
    distribution: DistributionMode,
    fix: bool,
) -> Result<DoctorReport, DoctorError> {
    run_doctor_with_fix_at_with_progress(repo_root, store_root, distribution, fix, &mut None)
}

pub(crate) fn run_doctor_with_fix_at_with_progress(
    repo_root: &Path,
    store_root: &Path,
    distribution: DistributionMode,
    fix: bool,
    reporter: &mut Option<&mut dyn ProgressReporter>,
) -> Result<DoctorReport, DoctorError> {
    if !fix {
        return run_doctor_at(repo_root, store_root, distribution);
    }

    let _ = emit_progress(reporter, ProgressKind::Stage, "Running diagnostics...");
    let report = run_doctor_at(repo_root, store_root, distribution)?;
    crate::doctor_fix::announce_and_apply_fixes(repo_root, distribution, &report, reporter);
    run_doctor_at(repo_root, store_root, distribution)
}

fn check_locks(store_paths: &StorePaths) -> Result<DoctorCheck, DoctorError> {
    let repo_lock_path = store_paths.repo_lock_path();
    let cache_lock_path = store_paths.cache_lock_path();

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

    Ok(DoctorCheck::simple("lock_health", status, detail))
}

fn check_worktree(
    repo_root: &Path,
    store_paths: &StorePaths,
    distribution: DistributionMode,
) -> DoctorCheck {
    if distribution != DistributionMode::Git {
        return DoctorCheck::simple(
            "worktree",
            DoctorStatus::Pass,
            "local-only mode; git worktree check skipped",
        );
    }
    if !repo_root.join(".git").exists() {
        return DoctorCheck::simple("worktree", DoctorStatus::Fail, "not a git repository");
    }

    let git = GitAdapter::new();
    let worktree = KnotsWorktree::with_store_paths(repo_root.to_path_buf(), store_paths);
    let result = worktree
        .ensure_exists(&git)
        .and_then(|()| worktree.ensure_clean(&git));

    match result {
        Ok(()) => DoctorCheck::simple("worktree", DoctorStatus::Pass, "knots worktree is clean"),
        Err(SyncError::DirtyWorktree(path)) => DoctorCheck::simple(
            "worktree",
            DoctorStatus::Fail,
            format!("worktree is dirty: {}", path.display()),
        ),
        Err(err) => DoctorCheck::simple(
            "worktree",
            DoctorStatus::Fail,
            format!("worktree check failed: {}", err),
        ),
    }
}

#[cfg(test)]
#[path = "doctor_workflows_tests.rs"]
mod doctor_workflows_tests;

fn check_remote(
    repo_root: &Path,
    distribution: DistributionMode,
) -> Result<DoctorCheck, DoctorError> {
    if distribution != DistributionMode::Git {
        return Ok(DoctorCheck::simple(
            "remote",
            DoctorStatus::Pass,
            "local-only mode; remote check skipped",
        ));
    }
    if !repo_root.join(".git").exists() {
        return Ok(DoctorCheck::simple(
            "remote",
            DoctorStatus::Fail,
            "not a git repository",
        ));
    }

    let config = SyncRefConfig::for_repo(repo_root);
    let remote = config.remote();
    let remote_ref = config.remote_ref();
    let remote_url = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["remote", "get-url", remote])
        .output()?;

    if !remote_url.status.success() {
        return Ok(DoctorCheck::simple(
            "remote",
            DoctorStatus::Fail,
            format!("remote '{remote}' is not configured"),
        ));
    }

    let ls_remote = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["ls-remote", "--exit-code", remote, remote_ref])
        .output()?;

    if ls_remote.status.success() {
        return Ok(DoctorCheck::simple(
            "remote",
            DoctorStatus::Pass,
            format!("{remote} reachable; knots ref {remote_ref} exists"),
        ));
    }

    if ls_remote.status.code() == Some(2) {
        return Ok(DoctorCheck::simple(
            "remote",
            DoctorStatus::Warn,
            format!("{remote} reachable; knots ref {remote_ref} missing (run `kno init`)"),
        ));
    }

    Ok(DoctorCheck::simple(
        "remote",
        DoctorStatus::Fail,
        format!(
            "{remote} is not reachable: {}",
            String::from_utf8_lossy(&ls_remote.stderr).trim()
        ),
    ))
}

fn check_hooks(repo_root: &Path, distribution: DistributionMode) -> DoctorCheck {
    if distribution != DistributionMode::Git {
        return DoctorCheck::simple(
            "hooks",
            DoctorStatus::Pass,
            "local-only mode; git hook check skipped",
        );
    }
    crate::git_hooks::check_hooks(repo_root)
}

const VERSION_CHECK_TIMEOUT_SECS: u32 = 5;

pub(crate) fn check_version() -> DoctorCheck {
    if crate::doctor_fix::version_fix_applied() {
        return DoctorCheck::simple(
            "version",
            DoctorStatus::Pass,
            "upgrade applied in this run; restart and rerun `kno doctor`",
        );
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
                Some(true) => DoctorCheck::simple(
                    "version",
                    DoctorStatus::Warn,
                    format!("update available: v{current} -> v{latest} (run `kno upgrade`)"),
                ),
                Some(false) => DoctorCheck::simple(
                    "version",
                    DoctorStatus::Pass,
                    format!("v{current} is up to date"),
                ),
                None => DoctorCheck::simple(
                    "version",
                    DoctorStatus::Warn,
                    format!("unable to compare v{current} with remote {tag}"),
                ),
            }
        }
        None => DoctorCheck::simple(
            "version",
            DoctorStatus::Warn,
            format!("v{current} (unable to check for updates)"),
        ),
    }
}

fn check_schema_version(store_paths: &StorePaths) -> Result<DoctorCheck, DoctorError> {
    let db_path = store_paths.db_path();
    if !db_path.exists() {
        return Ok(DoctorCheck::simple(
            "schema_version",
            DoctorStatus::Pass,
            "no cache database found",
        ));
    }
    let conn = crate::db::open_connection_raw(db_path.to_str().unwrap_or("cache/state.sqlite"))
        .map_err(|e| DoctorError::Io(std::io::Error::other(e.to_string())))?;

    let applied: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let expected = crate::db::CURRENT_SCHEMA_VERSION;
    if applied >= expected {
        Ok(DoctorCheck::simple(
            "schema_version",
            DoctorStatus::Pass,
            format!("schema version {applied} is current"),
        ))
    } else {
        Ok(DoctorCheck::simple(
            "schema_version",
            DoctorStatus::Warn,
            format!(
                "schema version {applied} is behind expected {expected} (run `kno doctor --fix`)"
            ),
        ))
    }
}

fn check_stuck_leases(store_paths: &StorePaths) -> Result<DoctorCheck, DoctorError> {
    let db_path = store_paths.db_path();
    if !db_path.exists() {
        return Ok(DoctorCheck::simple(
            "stuck_leases",
            DoctorStatus::Pass,
            "no cache database found",
        ));
    }
    let conn = crate::db::open_connection(db_path.to_str().unwrap_or("cache/state.sqlite"))
        .map_err(|e| DoctorError::Io(std::io::Error::other(e.to_string())))?;

    let count = crate::db::count_active_leases(&conn)
        .map_err(|e| DoctorError::Io(std::io::Error::other(e.to_string())))?;

    if count > 0 {
        Ok(DoctorCheck::simple(
            "stuck_leases",
            DoctorStatus::Warn,
            format!("{} active lease(s) may be stuck", count),
        ))
    } else {
        Ok(DoctorCheck::simple(
            "stuck_leases",
            DoctorStatus::Pass,
            "no stuck leases",
        ))
    }
}

fn check_terminal_parents(
    _repo_root: &Path,
    store_paths: &StorePaths,
) -> Result<DoctorCheck, DoctorError> {
    let db_path = store_paths.db_path();
    if !db_path.exists() {
        return Ok(DoctorCheck::simple(
            "terminal_parents",
            DoctorStatus::Pass,
            "no cache database found",
        ));
    }

    let conn = crate::db::open_connection(db_path.to_str().unwrap_or("cache/state.sqlite"))
        .map_err(|err| DoctorError::Io(std::io::Error::other(err.to_string())))?;
    let resolutions = find_terminal_parent_resolutions(&conn)
        .map_err(|err| DoctorError::Io(std::io::Error::other(err.to_string())))?;

    if resolutions.is_empty() {
        return Ok(DoctorCheck::simple(
            "terminal_parents",
            DoctorStatus::Pass,
            "no parent knots require terminal reconciliation",
        ));
    }

    let summary = resolutions
        .iter()
        .map(|resolution| format!("{} -> {}", resolution.parent.id, resolution.target_state))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(DoctorCheck::simple(
        "terminal_parents",
        DoctorStatus::Warn,
        format!(
            "{} parent knot(s) have only terminal children: {} (run `kno doctor --fix`)",
            resolutions.len(),
            summary
        ),
    ))
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

#[cfg(test)]
#[path = "doctor_progress_tests.rs"]
mod progress_tests;
