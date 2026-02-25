use std::path::PathBuf;

use crate::app::DEFAULT_WORKFLOW_META_KEY;
use crate::cli::{
    Commands, SelfArgs, SelfSubcommands, SelfUninstallArgs, SelfUpdateArgs, WorkflowArgs,
    WorkflowListArgs, WorkflowSetDefaultArgs, WorkflowShowArgs, WorkflowSubcommands,
};
use crate::db;
use crate::workflow::WorkflowRegistry;

use super::{
    knot_ref, maybe_run_self_command, read_repo_default_workflow_id, run_workflow_command,
};

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
        state: "work_item".to_string(),
        updated_at: "2026-02-25T10:00:00Z".to_string(),
        body: None,
        description: None,
        priority: None,
        knot_type: None,
        tags: Vec::new(),
        notes: Vec::new(),
        handoff_capsules: Vec::new(),
        workflow_id: "default".to_string(),
        workflow_etag: None,
        created_at: None,
    };
    assert_eq!(knot_ref(&with_alias), "A.1 (K-123)");

    let mut without_alias = with_alias;
    without_alias.alias = None;
    assert_eq!(knot_ref(&without_alias), "K-123");
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

    let self_update_outcome = maybe_run_self_command(&Commands::SelfManage(SelfArgs {
        command: SelfSubcommands::Update(SelfUpdateArgs {
            version: Some("v1.2.4".to_string()),
            repo: Some("acartine/knots".to_string()),
            install_dir: Some(dir.clone()),
            script_url,
        }),
    }))
    .expect("self update command should succeed")
    .expect("self update should emit summary");
    assert_eq!(self_update_outcome, "updated kno binary");

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
    let uninstall_self = maybe_run_self_command(&Commands::SelfManage(SelfArgs {
        command: SelfSubcommands::Uninstall(SelfUninstallArgs {
            bin_path: Some(binary.clone()),
            remove_previous: true,
        }),
    }))
    .expect("self uninstall should succeed")
    .expect("self uninstall should emit output");
    assert!(uninstall_self.contains("removed"));
    assert!(uninstall_self.contains("removed previous backups"));
    assert!(!binary.exists());
    assert!(!previous.exists());
    assert!(!legacy_previous.exists());

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn maybe_run_self_command_top_level_uninstall_reports_previous_backups_when_requested() {
    let dir = unique_dir("knots-main-top-uninstall");
    let binary = dir.join("knots");
    let previous = dir.join("kno.previous");
    let legacy_previous = dir.join("knots.previous");
    std::fs::write(&binary, b"bin").expect("binary should be writable");
    std::fs::write(&previous, b"bin").expect("previous backup should be writable");
    std::fs::write(&legacy_previous, b"bin").expect("legacy backup should be writable");

    let output = maybe_run_self_command(&Commands::Uninstall(SelfUninstallArgs {
        bin_path: Some(binary.clone()),
        remove_previous: true,
    }))
    .expect("top-level uninstall should succeed")
    .expect("top-level uninstall should return output");
    assert!(output.contains("removed previous backups"));
    assert!(!binary.exists());
    assert!(!previous.exists());
    assert!(!legacy_previous.exists());

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn read_repo_default_workflow_id_covers_missing_unknown_and_valid_values() {
    let root = unique_dir("knots-main-workflow-read");
    let db_path = root.join(".knots/cache/state.sqlite");
    let db_path_str = db_path.to_str().expect("utf8 db path").to_string();
    let registry = WorkflowRegistry::load().expect("workflow registry should load");

    let missing = read_repo_default_workflow_id(&db_path_str, &registry)
        .expect("missing db should return none");
    assert!(missing.is_none());

    std::fs::create_dir_all(db_path.parent().expect("db parent should exist"))
        .expect("db parent should be creatable");
    let conn = db::open_connection(&db_path_str).expect("db should open");

    let no_meta = read_repo_default_workflow_id(&db_path_str, &registry)
        .expect("db without meta should return none");
    assert!(no_meta.is_none());

    db::set_meta(&conn, DEFAULT_WORKFLOW_META_KEY, "missing-workflow").expect("meta should write");
    let unknown = read_repo_default_workflow_id(&db_path_str, &registry)
        .expect("unknown workflow should return none");
    assert!(unknown.is_none());

    db::set_meta(&conn, DEFAULT_WORKFLOW_META_KEY, "automation_granular")
        .expect("meta should write");
    let valid = read_repo_default_workflow_id(&db_path_str, &registry)
        .expect("valid workflow should return value");
    assert_eq!(valid.as_deref(), Some("automation_granular"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn run_workflow_command_handles_list_show_and_set_default_paths() {
    let root = unique_dir("knots-main-workflow-cmd");
    let db_path = root.join(".knots/cache/state.sqlite");
    let db_path_str = db_path.to_str().expect("utf8 db path").to_string();
    std::fs::create_dir_all(db_path.parent().expect("db parent should exist"))
        .expect("db parent should be creatable");
    let _ = db::open_connection(&db_path_str).expect("db should open");

    run_workflow_command(
        &WorkflowArgs {
            command: WorkflowSubcommands::List(WorkflowListArgs { json: false }),
        },
        &root,
        &db_path_str,
    )
    .expect("workflow list text path should succeed");

    run_workflow_command(
        &WorkflowArgs {
            command: WorkflowSubcommands::List(WorkflowListArgs { json: true }),
        },
        &root,
        &db_path_str,
    )
    .expect("workflow list json path should succeed");

    run_workflow_command(
        &WorkflowArgs {
            command: WorkflowSubcommands::Show(WorkflowShowArgs {
                id: "automation_granular".to_string(),
                json: false,
            }),
        },
        &root,
        &db_path_str,
    )
    .expect("workflow show text path should succeed");

    run_workflow_command(
        &WorkflowArgs {
            command: WorkflowSubcommands::Show(WorkflowShowArgs {
                id: "human_gate".to_string(),
                json: true,
            }),
        },
        &root,
        &db_path_str,
    )
    .expect("workflow show json path should succeed");

    run_workflow_command(
        &WorkflowArgs {
            command: WorkflowSubcommands::SetDefault(WorkflowSetDefaultArgs {
                id: "human_gate".to_string(),
            }),
        },
        &root,
        &db_path_str,
    )
    .expect("workflow set-default path should succeed");

    run_workflow_command(
        &WorkflowArgs {
            command: WorkflowSubcommands::List(WorkflowListArgs { json: false }),
        },
        &root,
        &db_path_str,
    )
    .expect("workflow list should include repo_default marker");

    let conn = db::open_connection(&db_path_str).expect("db should reopen");
    let stored =
        db::get_meta(&conn, DEFAULT_WORKFLOW_META_KEY).expect("default workflow meta should read");
    assert_eq!(stored.as_deref(), Some("human_gate"));

    let _ = std::fs::remove_dir_all(root);
}
