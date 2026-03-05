use std::path::PathBuf;
use std::process::Command;

use crate::cli::{Commands, HooksSubcommands, SelfUninstallArgs, SelfUpdateArgs};

use crate::dispatch::knot_ref;
use crate::self_manage::maybe_run_self_command;

fn unique_dir(prefix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("{}-{}", prefix, uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).expect("temp dir should be creatable");
    dir
}

fn run_git(root: &std::path::Path, args: &[&str]) {
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

fn setup_git_repo(prefix: &str) -> PathBuf {
    let root = unique_dir(prefix);
    run_git(&root, &["init"]);
    run_git(&root, &["config", "user.email", "knots@example.com"]);
    run_git(&root, &["config", "user.name", "Knots Test"]);
    std::fs::write(root.join("README.md"), "# test\n").expect("readme should be writable");
    run_git(&root, &["add", "README.md"]);
    run_git(&root, &["commit", "-m", "init"]);
    root
}

#[test]
fn knot_ref_prefers_alias_when_available() {
    let with_alias = crate::app::KnotView {
        id: "K-123".to_string(),
        alias: Some("A.1".to_string()),
        title: "t".to_string(),
        state: "ready_for_implementation".to_string(),
        updated_at: "2026-02-25T10:00:00Z".to_string(),
        body: None,
        description: None,
        priority: None,
        knot_type: crate::domain::knot_type::KnotType::default(),
        tags: Vec::new(),
        notes: Vec::new(),
        handoff_capsules: Vec::new(),
        invariants: Vec::new(),
        profile_id: "autopilot".to_string(),
        profile_etag: None,
        deferred_from_state: None,
        created_at: None,
    };
    assert_eq!(knot_ref(&with_alias), "A.1 (123)");

    let mut without_alias = with_alias;
    without_alias.alias = None;
    assert_eq!(knot_ref(&without_alias), "123");
}

#[test]
fn maybe_run_self_command_returns_none_for_non_self_commands() {
    let outcome = maybe_run_self_command(&Commands::Init).expect("init probe should succeed");
    assert!(outcome.is_none());
}

#[test]
fn maybe_run_self_command_update_and_uninstall_paths_execute() {
    let dir = unique_dir("knots-main-self-test");
    let script = dir.join("install.sh");
    std::fs::write(&script, "#!/bin/sh\nexit 0\n").expect("script should be writable");
    let script_url = format!("file://{}", script.display());

    let upgrade_outcome = maybe_run_self_command(&Commands::Upgrade(SelfUpdateArgs {
        version: Some("v1.2.3".to_string()),
        repo: Some("acartine/knots".to_string()),
        install_dir: Some(dir.clone()),
        script_url: script_url.clone(),
    }))
    .expect("upgrade command should succeed")
    .expect("upgrade should emit summary");
    assert!(upgrade_outcome.starts_with("Upgrade"));
    assert!(upgrade_outcome.contains("status:  updated kno binary"));
    assert!(upgrade_outcome.contains("version:  v1.2.3"));
    assert!(upgrade_outcome.contains("repo:  acartine/knots"));
    assert!(upgrade_outcome.contains("install_dir:  "));

    let second_upgrade_outcome = maybe_run_self_command(&Commands::Upgrade(SelfUpdateArgs {
        version: Some("v1.2.4".to_string()),
        repo: Some("acartine/knots".to_string()),
        install_dir: Some(dir.clone()),
        script_url,
    }))
    .expect("second upgrade command should succeed")
    .expect("second upgrade should emit summary");
    assert!(second_upgrade_outcome.starts_with("Upgrade"));
    assert!(second_upgrade_outcome.contains("status:  updated kno binary"));
    assert!(second_upgrade_outcome.contains("version:  v1.2.4"));

    let binary = dir.join("knots");
    let previous = dir.join("kno.previous");
    let legacy_previous = dir.join("knots.previous");
    std::fs::write(&binary, b"bin").expect("binary should be writable");
    std::fs::write(&previous, b"bin").expect("previous should be writable");
    std::fs::write(&legacy_previous, b"bin").expect("legacy previous should be writable");

    let uninstall_top = maybe_run_self_command(&Commands::Uninstall(SelfUninstallArgs {
        bin_path: Some(binary.clone()),
        remove_previous: false,
    }))
    .expect("top-level uninstall should succeed")
    .expect("top-level uninstall should emit output");
    assert!(uninstall_top.contains("removed"));
    assert!(!uninstall_top.contains("removed previous backups"));
    assert!(!binary.exists());
    assert!(previous.exists());
    assert!(legacy_previous.exists());

    std::fs::write(&binary, b"bin").expect("binary should be writable for second uninstall");
    let uninstall_with_previous = maybe_run_self_command(&Commands::Uninstall(SelfUninstallArgs {
        bin_path: Some(binary.clone()),
        remove_previous: true,
    }))
    .expect("second top-level uninstall should succeed")
    .expect("second top-level uninstall should emit output");
    assert!(uninstall_with_previous.contains("removed"));
    assert!(uninstall_with_previous.contains("removed previous backups"));
    assert!(!binary.exists());
    assert!(!previous.exists());
    assert!(!legacy_previous.exists());

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn run_hooks_command_handles_install_status_and_uninstall() {
    let root = setup_git_repo("knots-main-hooks-test");
    let pre_push = root.join(".git/hooks/pre-push");
    std::fs::create_dir_all(
        pre_push
            .parent()
            .expect("pre-push hook path should include parent directory"),
    )
    .expect("hooks directory should be creatable");
    std::fs::write(&pre_push, "#!/bin/sh\necho local-hook\n")
        .expect("local hook should be writable");

    super::run_hooks_command(&root, &HooksSubcommands::Install)
        .expect("hook install command should succeed");
    super::run_hooks_command(&root, &HooksSubcommands::Install)
        .expect("second hook install command should succeed");
    let installed = crate::git_hooks::hooks_status(&root);
    assert!(installed.hooks.iter().all(|(_, managed)| *managed));

    super::run_hooks_command(&root, &HooksSubcommands::Status)
        .expect("hook status command should succeed");

    super::run_hooks_command(&root, &HooksSubcommands::Uninstall)
        .expect("hook uninstall command should succeed");
    super::run_hooks_command(&root, &HooksSubcommands::Uninstall)
        .expect("second hook uninstall command should succeed");
    let uninstalled = crate::git_hooks::hooks_status(&root);
    assert!(uninstalled.hooks.iter().all(|(_, managed)| !*managed));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn run_git_panics_with_stderr_when_command_fails() {
    let root = unique_dir("knots-main-git-panic");
    let panic = std::panic::catch_unwind(|| run_git(&root, &["status"]));
    assert!(panic.is_err(), "run_git should panic for non-repo paths");
    let _ = std::fs::remove_dir_all(root);
}
