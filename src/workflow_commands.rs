use crate::app;
use crate::cli;
use crate::installed_workflows;

pub(crate) fn run_workflow_command(
    args: &cli::WorkflowArgs,
    repo_root: &std::path::Path,
) -> Result<(), app::AppError> {
    use cli::WorkflowSubcommands;

    match &args.command {
        WorkflowSubcommands::Install(install_args) => {
            let workflow_id = installed_workflows::install_bundle(repo_root, &install_args.source)?;
            println!("installed workflow: {workflow_id}");
        }
        WorkflowSubcommands::Use(use_args) => {
            let config = installed_workflows::set_current_workflow_selection(
                repo_root,
                &use_args.id,
                use_args.version,
                use_args.profile.as_deref(),
            )?;
            let workflow_id = config
                .current_workflow
                .unwrap_or_else(|| use_args.id.clone());
            if let Some(version) = config.current_version {
                if let Some(profile) = config.current_profile.as_deref() {
                    println!("current workflow: {workflow_id} v{version} profile={profile}");
                } else {
                    println!("current workflow: {workflow_id} v{version}");
                }
            } else {
                println!("current workflow: {workflow_id}");
            }
        }
        WorkflowSubcommands::Current(current_args) => {
            let registry = installed_workflows::InstalledWorkflowRegistry::load(repo_root)?;
            let workflow = registry.current_workflow()?;
            if current_args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "id": workflow.id,
                        "version": workflow.version,
                        "builtin": workflow.builtin,
                        "default_profile": workflow.default_profile,
                        "current_profile": registry
                            .current_profile_id()
                            .map(|profile| profile.rsplit('/').next().unwrap_or(profile)),
                    }))
                    .expect("json serialization should work")
                );
            } else {
                if let Some(profile) = registry.current_profile_id() {
                    let profile = profile.rsplit('/').next().unwrap_or(profile);
                    println!("{} v{} profile={profile}", workflow.id, workflow.version);
                } else {
                    println!("{} v{}", workflow.id, workflow.version);
                }
            }
        }
        WorkflowSubcommands::List(list_args) => {
            let registry = installed_workflows::InstalledWorkflowRegistry::load(repo_root)?;
            let current_id = registry.current_workflow_id().to_string();
            let current_version = registry.current_workflow_version();
            let workflows = registry
                .list()
                .into_iter()
                .map(|workflow| {
                    serde_json::json!({
                        "id": workflow.id,
                        "version": workflow.version,
                        "builtin": workflow.builtin,
                        "default_profile": workflow.default_profile,
                        "current": workflow.id == current_id
                            && Some(workflow.version) == current_version,
                        "profiles": workflow.profiles.keys().cloned().collect::<Vec<_>>(),
                    })
                })
                .collect::<Vec<_>>();
            if list_args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&workflows)
                        .expect("json serialization should work")
                );
            } else if workflows.is_empty() {
                println!("no workflows installed");
            } else {
                for item in workflows {
                    let id = item["id"].as_str().unwrap_or_default();
                    let version = item["version"].as_u64().unwrap_or_default();
                    let suffix = if item["current"].as_bool() == Some(true) {
                        " (current)"
                    } else {
                        ""
                    };
                    println!("{id} v{version}{suffix}");
                }
            }
        }
        WorkflowSubcommands::Show(show_args) => {
            let registry = installed_workflows::InstalledWorkflowRegistry::load(repo_root)?;
            let workflow = match show_args.version {
                Some(version) => registry.require_workflow_version(&show_args.id, version)?,
                None => registry.require_workflow(&show_args.id)?,
            };
            if show_args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(workflow).expect("json serialization should work")
                );
            } else {
                println!("workflow: {}", workflow.id);
                println!("version: {}", workflow.version);
                if let Some(description) = workflow.display_description() {
                    println!("description: {description}");
                }
                if let Some(default_profile) = workflow.default_profile.as_deref() {
                    println!("default_profile: {default_profile}");
                }
                println!("builtin: {}", workflow.builtin);
                println!("profiles:");
                for profile in workflow.profiles.keys() {
                    println!("  - {profile}");
                }
            }
        }
    }

    Ok(())
}
