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

use super::{build_version_check, check_version};

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
fn check_version_passes_when_upgrade_applied_in_process() {
    crate::doctor_fix::set_version_fix_applied_for_tests(true);
    let check = check_version();
    assert_eq!(check.status, DoctorStatus::Pass);
    assert!(check.detail.contains("upgrade applied in this run"));
    crate::doctor_fix::set_version_fix_applied_for_tests(false);
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
