use std::path::PathBuf;

use crate::cli::{Commands, SelfUninstallArgs, SelfUpdateArgs};

use super::{knot_ref, maybe_run_self_command};

fn unique_dir(prefix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("{}-{}", prefix, uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).expect("temp dir should be creatable");
    dir
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
        knot_type: None,
        tags: Vec::new(),
        notes: Vec::new(),
        handoff_capsules: Vec::new(),
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
    assert_eq!(upgrade_outcome, "updated kno binary");

    let second_upgrade_outcome = maybe_run_self_command(&Commands::Upgrade(SelfUpdateArgs {
        version: Some("v1.2.4".to_string()),
        repo: Some("acartine/knots".to_string()),
        install_dir: Some(dir.clone()),
        script_url,
    }))
    .expect("second upgrade command should succeed")
    .expect("second upgrade should emit summary");
    assert_eq!(second_upgrade_outcome, "updated kno binary");

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
