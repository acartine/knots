use std::path::{Path, PathBuf};
use std::process::Command;

use uuid::Uuid;

use crate::doctor::DoctorStatus;
use crate::git_hooks::{
    check_hooks, cleanup_legacy_hooks, hook_template, hooks_status,
    install_hooks, resolve_hooks_dir, uninstall_hooks,
    HookInstallOutcome, KNOTS_HOOK_MARKER,
};

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir()
        .join(format!("knots-git-hooks-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root)
        .expect("workspace should be creatable");
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

fn setup_git_repo() -> PathBuf {
    let root = unique_workspace();
    run_git(&root, &["init"]);
    run_git(
        &root,
        &["config", "user.email", "knots@example.com"],
    );
    run_git(&root, &["config", "user.name", "Knots Test"]);
    std::fs::write(root.join("README.md"), "# test\n")
        .expect("readme should write");
    run_git(&root, &["add", "README.md"]);
    run_git(&root, &["commit", "-m", "init"]);
    root
}

#[test]
fn resolve_hooks_dir_defaults_to_git_hooks() {
    let root = setup_git_repo();
    let dir = resolve_hooks_dir(&root);
    assert_eq!(dir, root.join(".git").join("hooks"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn resolve_hooks_dir_respects_core_hooks_path() {
    let root = setup_git_repo();
    let custom = root.join("custom-hooks");
    std::fs::create_dir_all(&custom).expect("custom hooks dir");
    run_git(
        &root,
        &[
            "config",
            "core.hooksPath",
            custom.to_str().expect("utf8"),
        ],
    );
    let dir = resolve_hooks_dir(&root);
    assert_eq!(dir, custom);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn install_hooks_creates_managed_hooks() {
    let root = setup_git_repo();
    let summary =
        install_hooks(&root).expect("install should succeed");
    assert_eq!(summary.outcomes.len(), 1);
    for (name, outcome) in &summary.outcomes {
        assert_eq!(*outcome, HookInstallOutcome::Installed);
        let path = root.join(".git").join("hooks").join(name);
        assert!(path.exists());
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains(KNOTS_HOOK_MARKER));
        assert!(contents.contains("kno pull"));
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn install_hooks_is_idempotent() {
    let root = setup_git_repo();
    install_hooks(&root).expect("first install");
    let summary = install_hooks(&root).expect("second install");
    for (_, outcome) in &summary.outcomes {
        assert_eq!(*outcome, HookInstallOutcome::AlreadyManaged);
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn install_hooks_preserves_existing_to_local() {
    let root = setup_git_repo();
    let hooks_dir = root.join(".git").join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();
    std::fs::write(
        hooks_dir.join("post-merge"),
        "#!/bin/sh\necho user hook\n",
    )
    .unwrap();

    let summary =
        install_hooks(&root).expect("install should succeed");
    let pm = summary
        .outcomes
        .iter()
        .find(|(n, _)| n == "post-merge")
        .unwrap();
    assert_eq!(pm.1, HookInstallOutcome::PreservedExisting);

    let local = hooks_dir.join("post-merge.local");
    assert!(local.exists());
    let local_contents = std::fs::read_to_string(&local).unwrap();
    assert!(local_contents.contains("echo user hook"));

    let managed =
        std::fs::read_to_string(hooks_dir.join("post-merge"))
            .unwrap();
    assert!(managed.contains(KNOTS_HOOK_MARKER));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn uninstall_hooks_removes_managed_and_restores_local() {
    let root = setup_git_repo();
    let hooks_dir = root.join(".git").join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();
    std::fs::write(
        hooks_dir.join("post-merge"),
        "#!/bin/sh\necho user hook\n",
    )
    .unwrap();

    install_hooks(&root).expect("install");
    let summary = uninstall_hooks(&root).expect("uninstall");

    let pm = summary
        .outcomes
        .iter()
        .find(|(n, _)| n == "post-merge")
        .unwrap();
    assert_eq!(pm.1, HookInstallOutcome::Installed);

    let restored =
        std::fs::read_to_string(hooks_dir.join("post-merge"))
            .unwrap();
    assert!(restored.contains("echo user hook"));
    assert!(!hooks_dir.join("post-merge.local").exists());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn uninstall_hooks_noop_when_not_installed() {
    let root = setup_git_repo();
    let summary = uninstall_hooks(&root).expect("uninstall");
    for (_, outcome) in &summary.outcomes {
        assert_eq!(*outcome, HookInstallOutcome::AlreadyManaged);
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn check_hooks_warns_when_missing() {
    let root = setup_git_repo();
    let check = check_hooks(&root);
    assert_eq!(check.name, "hooks");
    assert_eq!(check.status, DoctorStatus::Warn);
    assert!(check.detail.contains("missing sync hooks"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn check_hooks_passes_when_installed() {
    let root = setup_git_repo();
    install_hooks(&root).expect("install");
    let check = check_hooks(&root);
    assert_eq!(check.status, DoctorStatus::Pass);
    assert!(check.detail.contains("sync hooks installed"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn check_hooks_warns_for_non_git_directory() {
    let root = unique_workspace();
    let check = check_hooks(&root);
    assert_eq!(check.status, DoctorStatus::Warn);
    assert!(check.detail.contains("not a git repository"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn hooks_status_reports_installation_state() {
    let root = setup_git_repo();
    let before = hooks_status(&root);
    assert!(before.hooks.iter().all(|(_, managed)| !managed));

    install_hooks(&root).expect("install");
    let after = hooks_status(&root);
    assert!(after.hooks.iter().all(|(_, managed)| *managed));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn hook_template_contains_marker_and_sync() {
    let tmpl = hook_template("post-merge");
    assert!(tmpl.contains("knots-managed-post-merge-hook"));
    assert!(tmpl.contains("kno pull"));
    assert!(tmpl.contains("post-merge.local"));
    assert!(tmpl.starts_with("#!/usr/bin/env bash"));
}

#[test]
fn install_preserves_existing_to_backup_when_local_exists() {
    let root = setup_git_repo();
    let hooks_dir = root.join(".git").join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();
    std::fs::write(
        hooks_dir.join("post-merge"),
        "#!/bin/sh\necho original\n",
    )
    .unwrap();
    std::fs::write(
        hooks_dir.join("post-merge.local"),
        "#!/bin/sh\necho local\n",
    )
    .unwrap();

    let summary = install_hooks(&root).expect("install");
    let pm = summary
        .outcomes
        .iter()
        .find(|(n, _)| n == "post-merge")
        .unwrap();
    assert_eq!(pm.1, HookInstallOutcome::PreservedExisting);

    let local =
        std::fs::read_to_string(hooks_dir.join("post-merge.local"))
            .unwrap();
    assert!(local.contains("echo local"));

    let backups: Vec<_> = std::fs::read_dir(&hooks_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("post-merge.backup.")
        })
        .collect();
    assert_eq!(backups.len(), 1);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn check_hooks_warns_on_stale_content() {
    let root = setup_git_repo();
    install_hooks(&root).expect("install");
    let hooks_dir = root.join(".git").join("hooks");
    let old_template = format!(
        "#!/usr/bin/env bash\n\
         # {KNOTS_HOOK_MARKER}-post-merge-hook\n\
         kno sync >/dev/null 2>&1 &\n"
    );
    std::fs::write(
        hooks_dir.join("post-merge"),
        old_template,
    )
    .unwrap();

    let check = check_hooks(&root);
    assert_eq!(check.status, DoctorStatus::Warn);
    assert!(check.detail.contains("stale hook content"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn check_hooks_warns_on_legacy_hook() {
    let root = setup_git_repo();
    install_hooks(&root).expect("install");
    let hooks_dir = root.join(".git").join("hooks");
    let legacy = format!(
        "#!/usr/bin/env bash\n\
         # {KNOTS_HOOK_MARKER}-post-commit-hook\n\
         kno sync >/dev/null 2>&1 &\n"
    );
    std::fs::write(hooks_dir.join("post-commit"), legacy).unwrap();

    let check = check_hooks(&root);
    assert_eq!(check.status, DoctorStatus::Warn);
    assert!(check.detail.contains("orphaned legacy hooks"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cleanup_legacy_hooks_removes_orphaned_hook() {
    let root = setup_git_repo();
    let hooks_dir = root.join(".git").join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();
    let legacy = format!(
        "#!/usr/bin/env bash\n\
         # {KNOTS_HOOK_MARKER}-post-commit-hook\n\
         kno sync >/dev/null 2>&1 &\n"
    );
    std::fs::write(hooks_dir.join("post-commit"), legacy).unwrap();

    cleanup_legacy_hooks(&root);
    assert!(!hooks_dir.join("post-commit").exists());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cleanup_legacy_hooks_restores_local() {
    let root = setup_git_repo();
    let hooks_dir = root.join(".git").join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();
    let legacy = format!(
        "#!/usr/bin/env bash\n\
         # {KNOTS_HOOK_MARKER}-post-commit-hook\n\
         kno sync >/dev/null 2>&1 &\n"
    );
    std::fs::write(hooks_dir.join("post-commit"), legacy).unwrap();
    std::fs::write(
        hooks_dir.join("post-commit.local"),
        "#!/bin/sh\necho original\n",
    )
    .unwrap();

    cleanup_legacy_hooks(&root);
    let restored = std::fs::read_to_string(
        hooks_dir.join("post-commit"),
    )
    .unwrap();
    assert!(restored.contains("echo original"));
    assert!(!hooks_dir.join("post-commit.local").exists());
    let _ = std::fs::remove_dir_all(root);
}
