use std::path::Path;
use std::process::Command;

use crate::doctor::{DoctorCheck, DoctorStatus};
use crate::project::DistributionMode;
use crate::sync_ref::{SyncRefConfig, LEGACY_REMOTE_REF};

pub fn check_legacy_knots_head(repo_root: &Path, distribution: DistributionMode) -> DoctorCheck {
    if distribution != DistributionMode::Git {
        return DoctorCheck::simple(
            "legacy_knots_head",
            DoctorStatus::Pass,
            "local-only mode; legacy Knots head check skipped",
        );
    }
    if !repo_root.join(".git").exists() {
        return DoctorCheck::simple(
            "legacy_knots_head",
            DoctorStatus::Pass,
            "not a git repository; legacy Knots head check skipped",
        );
    }

    let config = SyncRefConfig::for_repo(repo_root);
    let Some(remote_url) = remote_url(repo_root, config.remote()) else {
        return DoctorCheck::simple(
            "legacy_knots_head",
            DoctorStatus::Pass,
            "remote unavailable; legacy Knots head check skipped",
        );
    };
    if !remote_url.contains("diffinite.sneka.ai") || config.remote_ref() == LEGACY_REMOTE_REF {
        return DoctorCheck::simple(
            "legacy_knots_head",
            DoctorStatus::Pass,
            "legacy Knots head is not stale for this remote",
        );
    }

    if remote_ref_exists(repo_root, config.remote(), LEGACY_REMOTE_REF) {
        DoctorCheck::simple(
            "legacy_knots_head",
            DoctorStatus::Warn,
            format!(
                "stale {LEGACY_REMOTE_REF} exists on {}; canonical ref is {}",
                config.remote(),
                config.remote_ref()
            ),
        )
    } else {
        DoctorCheck::simple(
            "legacy_knots_head",
            DoctorStatus::Pass,
            format!(
                "no stale {LEGACY_REMOTE_REF}; canonical ref is {}",
                config.remote_ref()
            ),
        )
    }
}

fn remote_url(repo_root: &Path, remote: &str) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["remote", "get-url", remote])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn remote_ref_exists(repo_root: &Path, remote: &str, remote_ref: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["ls-remote", "--exit-code", remote, remote_ref])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}
