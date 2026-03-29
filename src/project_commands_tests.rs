use std::fs;
use std::path::PathBuf;

use uuid::Uuid;

use crate::cli::{
    ProjectArgs, ProjectCreateArgs, ProjectDeleteArgs, ProjectListArgs, ProjectSubcommands,
    ProjectUseArgs,
};
use crate::project::{config_dir, create_named_project, read_global_config, NamedProjectRecord};
use crate::project_commands::{run_project_command, run_project_command_with_select_prompt};

fn temp_home(prefix: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7()));
    fs::create_dir_all(&path).expect("temp home should be creatable");
    path
}

#[test]
fn run_project_command_handles_create_use_clear_and_delete() {
    let home = temp_home("knots-project-command");
    let repo_root = home.join("workspace");
    fs::create_dir_all(&repo_root).expect("workspace should exist");

    run_project_command(
        &ProjectArgs {
            command: ProjectSubcommands::Create(ProjectCreateArgs {
                id: "demo".to_string(),
                repo_root: None,
                use_project: true,
            }),
        },
        Some(&home),
        Some(&repo_root),
    )
    .expect("create command should succeed");
    assert_eq!(
        read_global_config(Some(&home))
            .expect("config should load")
            .active_project
            .as_deref(),
        Some("demo")
    );

    run_project_command(
        &ProjectArgs {
            command: ProjectSubcommands::Use(ProjectUseArgs {
                id: "demo".to_string(),
            }),
        },
        Some(&home),
        None,
    )
    .expect("use command should succeed");

    run_project_command(
        &ProjectArgs {
            command: ProjectSubcommands::Clear,
        },
        Some(&home),
        None,
    )
    .expect("clear command should succeed");
    assert_eq!(
        read_global_config(Some(&home))
            .expect("config should load")
            .active_project,
        None
    );

    run_project_command(
        &ProjectArgs {
            command: ProjectSubcommands::Delete(ProjectDeleteArgs {
                id: "demo".to_string(),
                yes: true,
            }),
        },
        Some(&home),
        None,
    )
    .expect("delete command should succeed");
    assert!(!config_dir(Some(&home))
        .expect("config dir should resolve")
        .join("projects/demo.toml")
        .exists());

    let _ = fs::remove_dir_all(home);
}

#[test]
fn run_project_command_lists_empty_and_populated_projects() {
    let home = temp_home("knots-project-command-list");
    let repo_root = home.join("repo");
    fs::create_dir_all(&repo_root).expect("repo root should exist");

    run_project_command(
        &ProjectArgs {
            command: ProjectSubcommands::List(ProjectListArgs { json: false }),
        },
        Some(&home),
        None,
    )
    .expect("empty list should succeed");

    run_project_command(
        &ProjectArgs {
            command: ProjectSubcommands::Create(ProjectCreateArgs {
                id: "alpha".to_string(),
                repo_root: Some(repo_root.clone()),
                use_project: false,
            }),
        },
        Some(&home),
        None,
    )
    .expect("alpha create should succeed");
    run_project_command(
        &ProjectArgs {
            command: ProjectSubcommands::Create(ProjectCreateArgs {
                id: "beta".to_string(),
                repo_root: None,
                use_project: false,
            }),
        },
        Some(&home),
        None,
    )
    .expect("beta create should succeed");

    run_project_command(
        &ProjectArgs {
            command: ProjectSubcommands::List(ProjectListArgs { json: true }),
        },
        Some(&home),
        None,
    )
    .expect("json list should succeed");
    run_project_command(
        &ProjectArgs {
            command: ProjectSubcommands::List(ProjectListArgs { json: false }),
        },
        Some(&home),
        None,
    )
    .expect("text list should succeed");

    let _ = fs::remove_dir_all(home);
}

#[test]
fn run_project_command_handles_select_with_stub_prompt() {
    let home = temp_home("knots-project-command-select");
    let repo_root = home.join("repo");
    fs::create_dir_all(&repo_root).expect("repo root should exist");
    create_named_project(Some(&home), "beta", None).expect("beta should be creatable");

    let mut prompt = |_: Option<&std::path::Path>, current: Option<&std::path::Path>| {
        assert_eq!(current, Some(repo_root.as_path()));
        Ok(NamedProjectRecord {
            id: "beta".to_string(),
            repo_root: None,
        })
    };

    run_project_command_with_select_prompt(
        &ProjectArgs {
            command: ProjectSubcommands::Select,
        },
        Some(&home),
        Some(&repo_root),
        &mut prompt,
    )
    .expect("select command should succeed");

    assert_eq!(
        read_global_config(Some(&home))
            .expect("config should load")
            .active_project
            .as_deref(),
        Some("beta")
    );

    let _ = fs::remove_dir_all(home);
}
