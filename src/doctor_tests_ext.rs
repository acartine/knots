use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use uuid::Uuid;

use super::{check_version, run_doctor, wait_for_lock_release, DoctorError, DoctorStatus};

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-doctor-ext-{}", Uuid::now_v7()));
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

#[test]
fn doctor_error_display_source_and_from_cover_variants() {
    let io: DoctorError = std::io::Error::other("disk").into();
    assert!(io.to_string().contains("I/O error"));
    assert!(io.source().is_some());

    let lock: DoctorError = crate::locks::LockError::Busy(PathBuf::from("/tmp/lock")).into();
    assert!(lock.to_string().contains("lock error"));
    assert!(lock.source().is_some());
}

#[test]
fn remote_check_warns_when_knots_missing_and_passes_when_present() {
    let (root, local) = setup_repo_with_origin();

    let initial = run_doctor(&local).expect("doctor should run");
    let remote_initial = initial
        .checks
        .iter()
        .find(|check| check.name == "remote")
        .expect("remote check should exist");
    assert_eq!(remote_initial.status, DoctorStatus::Warn);
    assert!(remote_initial.detail.contains("knots branch missing"));

    run_git(&local, &["push", "origin", "HEAD:knots"]);
    let after = run_doctor(&local).expect("doctor should run after knots push");
    let remote_after = after
        .checks
        .iter()
        .find(|check| check.name == "remote")
        .expect("remote check should exist");
    assert_eq!(remote_after.status, DoctorStatus::Pass);
    assert!(remote_after.detail.contains("knots branch exists"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn remote_check_reports_unreachable_origin() {
    let root = unique_workspace();
    run_git(&root, &["init"]);
    run_git(&root, &["config", "user.email", "knots@example.com"]);
    run_git(&root, &["config", "user.name", "Knots Test"]);
    std::fs::write(root.join("README.md"), "# doctor\n").expect("readme should write");
    run_git(&root, &["add", "README.md"]);
    run_git(&root, &["commit", "-m", "init"]);
    run_git(
        &root,
        &["remote", "add", "origin", "file:///no/such/repo.git"],
    );

    let report = run_doctor(&root).expect("doctor should run");
    let remote = report
        .checks
        .iter()
        .find(|check| check.name == "remote")
        .expect("remote check should exist");
    assert_eq!(remote.status, DoctorStatus::Fail);
    assert!(remote.detail.contains("origin is not reachable"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn version_check_is_present_in_doctor_report() {
    let (root, local) = setup_repo_with_origin();

    let report = run_doctor(&local).expect("doctor should run");
    let version = report
        .checks
        .iter()
        .find(|check| check.name == "version")
        .expect("version check should exist");
    assert!(
        version.status == DoctorStatus::Pass || version.status == DoctorStatus::Warn,
        "version check should be pass or warn, got {:?}: {}",
        version.status,
        version.detail
    );
    assert!(
        version
            .detail
            .contains(&format!("v{}", env!("CARGO_PKG_VERSION"))),
        "detail should contain current version: {}",
        version.detail
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn check_version_returns_valid_doctor_check() {
    let check = check_version();
    assert_eq!(check.name, "version");
    assert!(check.status == DoctorStatus::Pass || check.status == DoctorStatus::Warn);
    assert!(check
        .detail
        .contains(&format!("v{}", env!("CARGO_PKG_VERSION"))));
}

#[test]
fn wait_for_lock_release_succeeds_for_unlocked_path() {
    let root = unique_workspace();
    let lock_path = root.join(".knots/locks/repo.lock");
    let unlocked = wait_for_lock_release(&lock_path, Duration::from_millis(20))
        .expect("lock release probe should succeed");
    assert!(unlocked);

    let _ = std::fs::remove_dir_all(root);
}
