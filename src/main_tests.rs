use std::path::PathBuf;

use crate::cli::{
    Commands, ProfileArgs, ProfileListArgs, ProfileSetArgs, ProfileSetDefaultArgs, ProfileShowArgs,
    ProfileSubcommands, SelfUninstallArgs, SelfUpdateArgs,
};
use crate::workflow::OutputMode;

use super::{
    format_profile_fields, format_profile_output_mode, knot_ref, maybe_run_self_command,
    resolve_profile_state_selection, run_profile_command, ProfileField, ProfilePalette,
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
    assert_eq!(knot_ref(&with_alias), "A.1 (K-123)");

    let mut without_alias = with_alias;
    without_alias.alias = None;
    assert_eq!(knot_ref(&without_alias), "K-123");
}

#[test]
fn profile_field_formatting_right_aligns_labels() {
    let palette = ProfilePalette { enabled: false };
    let fields = vec![
        ProfileField::new("id", "autopilot"),
        ProfileField::new("terminal_states", "shipped, abandoned"),
    ];
    let lines = format_profile_fields(&fields, &palette);
    assert_eq!(lines[0], "             id:  autopilot");
    assert_eq!(lines[1], "terminal_states:  shipped, abandoned");
}

#[test]
fn profile_output_mode_labels_remote_main_as_merged() {
    assert_eq!(
        format_profile_output_mode(&OutputMode::RemoteMain),
        "RemoteMain (merged)"
    );
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

#[test]
fn run_profile_command_handles_list_show_and_set_default() {
    let root = unique_dir("knots-main-profile-cmd");
    let db_path = root.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(db_path.parent().expect("db parent should exist"))
        .expect("db parent should be creatable");
    let db_path_str = db_path.to_str().expect("utf8 db path").to_string();

    run_profile_command(
        &ProfileArgs {
            command: ProfileSubcommands::List(ProfileListArgs { json: false }),
        },
        &root,
        &db_path_str,
    )
    .expect("profile list text path should succeed");

    run_profile_command(
        &ProfileArgs {
            command: ProfileSubcommands::List(ProfileListArgs { json: true }),
        },
        &root,
        &db_path_str,
    )
    .expect("profile list json path should succeed");

    run_profile_command(
        &ProfileArgs {
            command: ProfileSubcommands::Show(ProfileShowArgs {
                id: "autopilot".to_string(),
                json: false,
            }),
        },
        &root,
        &db_path_str,
    )
    .expect("profile show text path should succeed");

    run_profile_command(
        &ProfileArgs {
            command: ProfileSubcommands::Show(ProfileShowArgs {
                id: "semiauto".to_string(),
                json: true,
            }),
        },
        &root,
        &db_path_str,
    )
    .expect("profile show json path should succeed");

    let prev_home = std::env::var_os("HOME");
    unsafe {
        std::env::set_var("HOME", &root);
    }
    run_profile_command(
        &ProfileArgs {
            command: ProfileSubcommands::SetDefault(ProfileSetDefaultArgs {
                id: "semiauto".to_string(),
            }),
        },
        &root,
        &db_path_str,
    )
    .expect("profile set-default should succeed");
    if let Some(home) = prev_home {
        unsafe {
            std::env::set_var("HOME", home);
        }
    } else {
        unsafe {
            std::env::remove_var("HOME");
        }
    }

    let config_path = root.join(".config/knots/config.toml");
    let config = std::fs::read_to_string(config_path).expect("config should be readable");
    assert!(config.contains("default_profile = \"semiauto\""));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn profile_set_requires_state_in_non_interactive_mode() {
    let root = unique_dir("knots-main-profile-set");
    let db_path = root.join(".knots/cache/state.sqlite");
    let db_path_str = db_path.to_str().expect("utf8 db path").to_string();

    let app =
        crate::app::App::open(&db_path_str, root.clone()).expect("app should open for fixture");
    let created = app
        .create_knot("Profile Switch", None, Some("idea"), Some("autopilot"))
        .expect("fixture knot should be created");

    let missing_state = run_profile_command(
        &ProfileArgs {
            command: ProfileSubcommands::Set(ProfileSetArgs {
                id: created.id.clone(),
                profile: "autopilot_no_planning".to_string(),
                state: None,
                if_match: None,
            }),
        },
        &root,
        &db_path_str,
    );
    assert!(matches!(
        missing_state,
        Err(crate::app::AppError::InvalidArgument(_))
    ));

    run_profile_command(
        &ProfileArgs {
            command: ProfileSubcommands::Set(ProfileSetArgs {
                id: created.id.clone(),
                profile: "autopilot_no_planning".to_string(),
                state: Some("ready_for_implementation".to_string()),
                if_match: None,
            }),
        },
        &root,
        &db_path_str,
    )
    .expect("profile set with explicit state should succeed");

    let updated = app
        .show_knot(&created.id)
        .expect("show should succeed")
        .expect("knot should exist");
    assert_eq!(updated.profile_id, "autopilot_no_planning");
    assert_eq!(updated.state, "ready_for_implementation");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn resolve_profile_state_selection_handles_non_interactive_paths() {
    let registry = crate::workflow::WorkflowRegistry::load().expect("registry should load");
    let profile = registry
        .require("autopilot_no_planning")
        .expect("profile should exist");

    let valid =
        resolve_profile_state_selection(profile, Some("work_item"), "ready_for_implementation")
            .expect("legacy state alias should normalize");
    assert_eq!(valid, "ready_for_implementation");

    let invalid =
        resolve_profile_state_selection(profile, Some("plan_review"), "ready_for_implementation")
            .expect_err("state outside profile should fail");
    assert!(matches!(invalid, crate::app::AppError::InvalidArgument(_)));
}
