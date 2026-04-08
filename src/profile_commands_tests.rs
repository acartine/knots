use std::path::PathBuf;

use std::collections::BTreeMap;

use crate::cli::{
    ProfileArgs, ProfileListArgs, ProfileSetArgs, ProfileSetDefaultArgs, ProfileShowArgs,
    ProfileSubcommands,
};
use crate::workflow::ActionOutputDef;

use super::*;

fn unique_dir(prefix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("{}-{}", prefix, uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).expect("temp dir should be creatable");
    crate::installed_workflows::ensure_builtin_workflows_registered(&dir)
        .expect("builtin workflows should register");
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
fn profile_outputs_formats_unique_artifact_types() {
    let outputs = BTreeMap::from([
        (
            "planning".to_string(),
            ActionOutputDef {
                artifact_type: "note".to_string(),
                access_hint: Some("kno show".to_string()),
            },
        ),
        (
            "implementation".to_string(),
            ActionOutputDef {
                artifact_type: "branch".to_string(),
                access_hint: None,
            },
        ),
        (
            "shipment".to_string(),
            ActionOutputDef {
                artifact_type: "branch".to_string(),
                access_hint: None,
            },
        ),
    ]);
    assert_eq!(format_profile_outputs(&outputs), "branch, note");

    let empty: BTreeMap<String, ActionOutputDef> = BTreeMap::new();
    assert_eq!(format_profile_outputs(&empty), "(none)");

    assert_eq!(
        format_profile_gate_mode(&crate::workflow::GateMode::Optional),
        "Optional"
    );
    assert_eq!(
        format_profile_gate_mode(&crate::workflow::GateMode::Skipped),
        "Skipped"
    );
}

#[test]
fn profile_helpers_cover_empty_fields_and_enabled_palette_paths() {
    let lines = format_profile_fields(&[], &ProfilePalette { enabled: false });
    assert!(lines.is_empty());

    let palette = ProfilePalette { enabled: true };
    assert_eq!(palette.label("id"), "\u{1b}[36mid\u{1b}[0m");
    assert_eq!(palette.heading("Profile"), "\u{1b}[1;36mProfile\u{1b}[0m");
    assert_eq!(palette.dim("muted"), "\u{1b}[2mmuted\u{1b}[0m");
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

    run_profile_command_with_home(
        &ProfileArgs {
            command: ProfileSubcommands::SetDefault(ProfileSetDefaultArgs {
                id: "semiauto".to_string(),
            }),
        },
        &root,
        &db_str,
        Some(root.clone()),
    )
    .expect("profile set-default should succeed");

    let workflow_config_path = root.join(".knots/workflows/current");
    let workflow_config =
        std::fs::read_to_string(&workflow_config_path).expect("workflow config should be readable");
    assert!(workflow_config.contains("semiauto"));

    run_profile_command_with_home(
        &ProfileArgs {
            command: ProfileSubcommands::SetDefaultQuick(ProfileSetDefaultArgs {
                id: "autopilot_no_planning".to_string(),
            }),
        },
        &root,
        &db_str,
        Some(root.clone()),
    )
    .expect("profile set-default-quick should succeed");

    let config_path = root.join(".config/knots/config.toml");
    let config2 = std::fs::read_to_string(&config_path).expect("config should be readable");
    assert!(
        config2.contains("default_quick_profile"),
        "config should contain default_quick_profile: {config2}"
    );
    assert!(
        config2.contains("autopilot_no_planning"),
        "config should preserve quick profile id: {config2}"
    );
    assert!(
        workflow_config.contains("semiauto"),
        "workflow config should still contain default profile: {workflow_config}"
    );

    let app = crate::app::App::open(&db_str, root.clone())
        .expect("app should open")
        .with_home_override(Some(root.clone()));
    let quick = app
        .default_quick_profile_id()
        .expect("should read quick default");
    assert_eq!(quick, "autopilot_no_planning");

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
fn profile_set_formats_alias_when_available() {
    let root = unique_dir("knots-profcmd-alias");
    let db_path = root.join(".knots/cache/state.sqlite");
    let db_str = db_path.to_str().expect("utf8 db path").to_string();

    let app = crate::app::App::open(&db_str, root.clone()).expect("app should open");
    let parent = app
        .create_knot("Parent", None, Some("planning"), Some("autopilot"))
        .expect("parent should exist");
    let child = app
        .create_knot("Child", None, Some("planning"), Some("autopilot"))
        .expect("child should exist");
    app.add_edge(&parent.id, "parent_of", &child.id)
        .expect("edge should add");

    run_profile_command(
        &ProfileArgs {
            command: ProfileSubcommands::Set(ProfileSetArgs {
                id: child.id.clone(),
                profile: "autopilot_no_planning".to_string(),
                state: Some("ready_for_implementation".to_string()),
                if_match: None,
            }),
        },
        &root,
        &db_str,
    )
    .expect("profile set should succeed with alias");

    let updated = app
        .show_knot(&child.id)
        .expect("show should succeed")
        .expect("child should exist");
    assert!(updated.alias.is_some());

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
