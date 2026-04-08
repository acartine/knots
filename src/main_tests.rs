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

fn managed_hook_path(root: &std::path::Path) -> PathBuf {
    root.join(".git")
        .join("hooks")
        .join(crate::git_hooks::MANAGED_HOOKS[0])
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
        acceptance: None,
        priority: None,
        knot_type: crate::domain::knot_type::KnotType::default(),
        tags: Vec::new(),
        notes: Vec::new(),
        handoff_capsules: Vec::new(),
        invariants: Vec::new(),
        step_history: Vec::new(),
        gate: None,
        lease: None,
        lease_id: None,
        lease_expiry_ts: 0,
        lease_agent: None,
        workflow_id: "knots_sdlc".to_string(),
        profile_id: "autopilot".to_string(),
        profile_etag: None,
        deferred_from_state: None,
        blocked_from_state: None,
        created_at: None,
        step_metadata: None,
        next_step_metadata: None,
        edges: Vec::new(),
        child_summaries: vec![],
    };
    assert_eq!(knot_ref(&with_alias), "A.1 (123)");

    let mut without_alias = with_alias;
    without_alias.alias = None;
    assert_eq!(knot_ref(&without_alias), "123");
}

#[test]
fn maybe_run_self_command_returns_none_for_non_self_commands() {
    let cwd = std::env::current_dir().expect("cwd should resolve");
    let outcome = maybe_run_self_command(&Commands::Init, &cwd).expect("init probe should succeed");
    assert!(outcome.is_none());
}

#[test]
fn maybe_run_self_command_update_and_uninstall_paths_execute() {
    let dir = unique_dir("knots-main-self-test");
    let script = dir.join("install.sh");
    std::fs::write(&script, "#!/bin/sh\nexit 0\n").expect("script should be writable");
    let script_url = format!("file://{}", script.display());

    let upgrade_outcome = maybe_run_self_command(
        &Commands::Upgrade(SelfUpdateArgs {
            version: Some("v1.2.3".to_string()),
            repo: Some("acartine/knots".to_string()),
            install_dir: Some(dir.clone()),
            script_url: script_url.clone(),
        }),
        &dir,
    )
    .expect("upgrade command should succeed")
    .expect("upgrade should emit summary");
    assert!(upgrade_outcome.starts_with("Upgrade"));
    assert!(upgrade_outcome.contains("status:  updated kno binary"));
    assert!(upgrade_outcome.contains("version:  v1.2.3"));
    assert!(upgrade_outcome.contains("repo:  acartine/knots"));
    assert!(upgrade_outcome.contains("install_dir:  "));

    let second_upgrade_outcome = maybe_run_self_command(
        &Commands::Upgrade(SelfUpdateArgs {
            version: Some("v1.2.4".to_string()),
            repo: Some("acartine/knots".to_string()),
            install_dir: Some(dir.clone()),
            script_url,
        }),
        &dir,
    )
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

    let uninstall_top = maybe_run_self_command(
        &Commands::Uninstall(SelfUninstallArgs {
            bin_path: Some(binary.clone()),
            remove_previous: false,
        }),
        &dir,
    )
    .expect("top-level uninstall should succeed")
    .expect("top-level uninstall should emit output");
    assert!(uninstall_top.contains("removed"));
    assert!(!uninstall_top.contains("removed previous backups"));
    assert!(!binary.exists());
    assert!(previous.exists());
    assert!(legacy_previous.exists());

    std::fs::write(&binary, b"bin").expect("binary should be writable for second uninstall");
    let uninstall_with_previous = maybe_run_self_command(
        &Commands::Uninstall(SelfUninstallArgs {
            bin_path: Some(binary.clone()),
            remove_previous: true,
        }),
        &dir,
    )
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
fn maybe_run_self_command_upgrade_hint_tracks_hook_health() {
    let dir = unique_dir("knots-main-self-upgrade-hooks");
    let script = dir.join("install.sh");
    std::fs::write(&script, "#!/bin/sh\nexit 0\n").expect("script should be writable");
    let script_url = format!("file://{}", script.display());

    let clean_repo = setup_git_repo("knots-main-self-upgrade-clean");
    crate::git_hooks::install_hooks(&clean_repo).expect("hooks should install");
    let clean_outcome = maybe_run_self_command(
        &Commands::Upgrade(SelfUpdateArgs {
            version: None,
            repo: None,
            install_dir: Some(dir.clone()),
            script_url: script_url.clone(),
        }),
        &clean_repo,
    )
    .expect("clean upgrade should succeed")
    .expect("clean upgrade should emit summary");
    assert!(!clean_outcome.contains("kno doctor"));

    let missing_repo = setup_git_repo("knots-main-self-upgrade-missing");
    let missing_outcome = maybe_run_self_command(
        &Commands::Upgrade(SelfUpdateArgs {
            version: None,
            repo: None,
            install_dir: Some(dir.clone()),
            script_url: script_url.clone(),
        }),
        &missing_repo,
    )
    .expect("missing-hook upgrade should succeed")
    .expect("missing-hook upgrade should emit summary");
    assert!(missing_outcome.contains("kno doctor"));

    let stale_repo = setup_git_repo("knots-main-self-upgrade-stale");
    crate::git_hooks::install_hooks(&stale_repo).expect("hooks should install");
    std::fs::write(
        managed_hook_path(&stale_repo),
        "#!/usr/bin/env bash\n# stale\n",
    )
    .expect("stale hook fixture should be writable");
    let stale_outcome = maybe_run_self_command(
        &Commands::Upgrade(SelfUpdateArgs {
            version: None,
            repo: None,
            install_dir: Some(dir.clone()),
            script_url,
        }),
        &stale_repo,
    )
    .expect("stale-hook upgrade should succeed")
    .expect("stale-hook upgrade should emit summary");
    assert!(stale_outcome.contains("kno doctor"));

    let _ = std::fs::remove_dir_all(clean_repo);
    let _ = std::fs::remove_dir_all(missing_repo);
    let _ = std::fs::remove_dir_all(stale_repo);
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

#[test]
fn knot_json_serialization_always_includes_step_history_field() {
    let knot = crate::app::KnotView {
        id: "K-200".to_string(),
        alias: None,
        title: "json field test".to_string(),
        state: "ready_for_implementation".to_string(),
        updated_at: "2026-03-06T00:00:00Z".to_string(),
        body: None,
        description: None,
        acceptance: None,
        priority: None,
        knot_type: crate::domain::knot_type::KnotType::default(),
        tags: Vec::new(),
        notes: Vec::new(),
        handoff_capsules: Vec::new(),
        invariants: Vec::new(),
        step_history: Vec::new(),
        gate: None,
        lease: None,
        lease_id: None,
        lease_expiry_ts: 0,
        lease_agent: None,
        workflow_id: "knots_sdlc".to_string(),
        profile_id: "autopilot".to_string(),
        profile_etag: None,
        deferred_from_state: None,
        blocked_from_state: None,
        created_at: None,
        step_metadata: None,
        next_step_metadata: None,
        edges: Vec::new(),
        child_summaries: vec![],
    };

    let json = serde_json::to_value(&knot).expect("serialize knot");
    assert!(
        json.get("step_history").is_some(),
        "step_history must be present in canonical show JSON"
    );
    assert_eq!(
        json["step_history"].as_array().map(std::vec::Vec::len),
        Some(0),
        "step_history should serialize as an empty array when no steps exist"
    );
}

#[test]
fn format_error_appends_worktree_hint_for_not_found() {
    let err = crate::app::AppError::NotFound("abc123".to_string());
    let output = super::format_error(&err);
    assert!(
        output.contains("knot 'abc123' not found"),
        "should include the standard not-found message"
    );
    assert!(
        output.contains("kno -C <repo_root>"),
        "should include the worktree recovery hint"
    );
}

#[test]
fn format_error_no_worktree_hint_for_other_errors() {
    let err = crate::app::AppError::InvalidArgument("bad arg".to_string());
    let output = super::format_error(&err);
    assert!(
        output.contains("bad arg"),
        "should include the original error message"
    );
    assert!(
        !output.contains("worktree"),
        "should not include worktree hint for non-NotFound errors"
    );
}

#[test]
fn format_error_not_found_preserves_knot_id() {
    let err = crate::app::AppError::NotFound("knots-xyz9".to_string());
    let output = super::format_error(&err);
    assert!(
        output.contains("knots-xyz9"),
        "should preserve the knot ID in the error output"
    );
}
