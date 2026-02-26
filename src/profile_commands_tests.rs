use std::path::PathBuf;

use crate::cli::{
    ProfileArgs, ProfileListArgs, ProfileSetArgs, ProfileSetDefaultArgs, ProfileShowArgs,
    ProfileSubcommands,
};
use crate::workflow::OutputMode;

use super::*;

fn unique_dir(prefix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("{}-{}", prefix, uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).expect("temp dir should be creatable");
    dir
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
fn run_profile_command_handles_list_show_and_set_default() {
    let root = unique_dir("knots-profcmd-test");
    let db_path = root.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(db_path.parent().expect("db parent should exist"))
        .expect("db parent should be creatable");
    let db_str = db_path.to_str().expect("utf8 db path").to_string();

    run_profile_command(
        &ProfileArgs {
            command: ProfileSubcommands::List(ProfileListArgs { json: false }),
        },
        &root,
        &db_str,
    )
    .expect("profile list text path should succeed");

    run_profile_command(
        &ProfileArgs {
            command: ProfileSubcommands::List(ProfileListArgs { json: true }),
        },
        &root,
        &db_str,
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
        &db_str,
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
        &db_str,
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
        &db_str,
    )
    .expect("profile set-default should succeed");

    let config_path = root.join(".config/knots/config.toml");
    let config = std::fs::read_to_string(&config_path).expect("config should be readable");
    assert!(config.contains("semiauto"));

    // Also test set-default-quick while HOME is overridden
    run_profile_command(
        &ProfileArgs {
            command: ProfileSubcommands::SetDefaultQuick(ProfileSetDefaultArgs {
                id: "autopilot_no_planning".to_string(),
            }),
        },
        &root,
        &db_str,
    )
    .expect("profile set-default-quick should succeed");

    let config2 = std::fs::read_to_string(&config_path).expect("config should be readable");
    assert!(
        config2.contains("default_quick_profile"),
        "config should contain default_quick_profile: {config2}"
    );
    assert!(
        config2.contains("autopilot_no_planning"),
        "config should preserve quick profile id: {config2}"
    );
    // Verify both defaults coexist
    assert!(
        config2.contains("semiauto"),
        "config should still contain default_profile: {config2}"
    );

    // Verify App reads it back correctly
    let app = crate::app::App::open(&db_str, root.clone()).expect("app should open");
    let quick = app
        .default_quick_profile_id()
        .expect("should read quick default");
    assert_eq!(quick, "autopilot_no_planning");

    if let Some(home) = prev_home {
        unsafe {
            std::env::set_var("HOME", home);
        }
    } else {
        unsafe {
            std::env::remove_var("HOME");
        }
    }

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn profile_set_requires_state_in_non_interactive_mode() {
    let root = unique_dir("knots-profcmd-set");
    let db_path = root.join(".knots/cache/state.sqlite");
    let db_str = db_path.to_str().expect("utf8 db path").to_string();

    let app = crate::app::App::open(&db_str, root.clone()).expect("app should open for fixture");
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
        &db_str,
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
        &db_str,
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
fn resolve_profile_state_handles_non_interactive_paths() {
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
