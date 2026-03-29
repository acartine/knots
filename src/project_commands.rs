use std::io::{self, Write};

use serde::Serialize;

use crate::app::AppError;
use crate::cli::{
    ProjectArgs, ProjectCreateArgs, ProjectDeleteArgs, ProjectListArgs, ProjectSubcommands,
};
use crate::project;

#[derive(Debug, Serialize)]
struct ProjectListEntry {
    id: String,
    repo_root: Option<String>,
}

pub(crate) fn run_project_command(
    args: &ProjectArgs,
    home_override: Option<&std::path::Path>,
    current_repo_root: Option<&std::path::Path>,
) -> Result<(), AppError> {
    run_project_command_with_select_prompt(
        args,
        home_override,
        current_repo_root,
        &mut project::prompt_for_project_selection,
    )
}

pub(crate) fn run_project_command_with_select_prompt<F>(
    args: &ProjectArgs,
    home_override: Option<&std::path::Path>,
    current_repo_root: Option<&std::path::Path>,
    select_prompt: &mut F,
) -> Result<(), AppError>
where
    F: FnMut(
        Option<&std::path::Path>,
        Option<&std::path::Path>,
    ) -> Result<project::NamedProjectRecord, String>,
{
    match &args.command {
        ProjectSubcommands::Create(create) => {
            create_project(create, home_override, current_repo_root)
        }
        ProjectSubcommands::Delete(delete) => delete_project(delete, home_override),
        ProjectSubcommands::Use(use_args) => use_project(&use_args.id, home_override),
        ProjectSubcommands::Clear => clear_project(home_override),
        ProjectSubcommands::List(list) => list_projects(list, home_override),
        ProjectSubcommands::Select => {
            select_project(home_override, current_repo_root, select_prompt)
        }
    }
}

fn delete_project(
    args: &ProjectDeleteArgs,
    home_override: Option<&std::path::Path>,
) -> Result<(), AppError> {
    let project =
        project::load_named_project(home_override, &args.id).map_err(AppError::InvalidArgument)?;
    confirm_project_delete(&project.id, args.yes)?;
    project::delete_named_project(home_override, &project.id).map_err(AppError::InvalidArgument)?;
    println!("deleted {}", project.id);
    Ok(())
}

fn create_project(
    args: &ProjectCreateArgs,
    home_override: Option<&std::path::Path>,
    current_repo_root: Option<&std::path::Path>,
) -> Result<(), AppError> {
    let repo_root = args.repo_root.as_deref().or(current_repo_root);
    let project = project::create_named_project(home_override, &args.id, repo_root)
        .map_err(AppError::InvalidArgument)?;
    if args.use_project {
        project::set_active_project(home_override, &project.id)
            .map_err(AppError::InvalidArgument)?;
        println!("created and activated {}", project.id);
    } else {
        println!("created {}", project.id);
    }
    Ok(())
}

fn use_project(id: &str, home_override: Option<&std::path::Path>) -> Result<(), AppError> {
    project::set_active_project(home_override, id).map_err(AppError::InvalidArgument)?;
    println!("active project: {}", id);
    Ok(())
}

fn clear_project(home_override: Option<&std::path::Path>) -> Result<(), AppError> {
    project::clear_active_project(home_override).map_err(AppError::InvalidArgument)?;
    println!("active project cleared");
    Ok(())
}

pub(crate) fn select_project<F>(
    home_override: Option<&std::path::Path>,
    current_repo_root: Option<&std::path::Path>,
    select_prompt: &mut F,
) -> Result<(), AppError>
where
    F: FnMut(
        Option<&std::path::Path>,
        Option<&std::path::Path>,
    ) -> Result<project::NamedProjectRecord, String>,
{
    let selected =
        select_prompt(home_override, current_repo_root).map_err(AppError::InvalidArgument)?;
    use_project(&selected.id, home_override)
}

fn confirm_project_delete(id: &str, skip_prompt: bool) -> Result<(), AppError> {
    if skip_prompt {
        return Ok(());
    }

    eprintln!("Delete project '{id}'? This removes its local Knots store and metadata.");
    eprint!("Type the project id to confirm: ");
    io::stderr().flush()?;

    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    if line.trim() == id {
        return Ok(());
    }

    Err(AppError::InvalidArgument(format!(
        "confirmation did not match project id '{id}'"
    )))
}

fn list_projects(
    args: &ProjectListArgs,
    home_override: Option<&std::path::Path>,
) -> Result<(), AppError> {
    let projects =
        project::list_named_projects(home_override).map_err(AppError::InvalidArgument)?;
    if args.json {
        let rows: Vec<ProjectListEntry> = projects
            .into_iter()
            .map(|project| ProjectListEntry {
                id: project.id,
                repo_root: project.repo_root.map(|path| path.display().to_string()),
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&rows).expect("json serialization should succeed")
        );
        return Ok(());
    }
    if projects.is_empty() {
        println!("no named projects");
        return Ok(());
    }
    for project in projects {
        match project.repo_root {
            Some(repo_root) => println!("{}  {}", project.id, repo_root.display()),
            None => println!("{}", project.id),
        }
    }
    Ok(())
}
