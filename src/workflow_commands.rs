use crate::app;
use crate::cli;
use crate::domain::knot_type::KnotType;
use crate::installed_workflows;
use std::io::{self, BufRead, IsTerminal, Write};

fn parse_bool_flag(raw: &str) -> Result<bool, app::AppError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "yes" | "true" | "1" => Ok(true),
        "no" | "false" | "0" => Ok(false),
        other => Err(app::AppError::InvalidArgument(format!(
            "invalid boolean value '{}'; expected yes|true|1|no|false|0",
            other
        ))),
    }
}

fn prompt_install_default(workflow_id: &str) -> Result<bool, app::AppError> {
    if !io::stdin().is_terminal() {
        return Ok(false);
    }
    print!("set '{workflow_id}' as the default workflow? [y/N]: ");
    io::stdout()
        .flush()
        .map_err(|err| app::AppError::InvalidArgument(err.to_string()))?;
    let mut input = String::new();
    io::stdin()
        .lock()
        .read_line(&mut input)
        .map_err(|err| app::AppError::InvalidArgument(err.to_string()))?;
    Ok(matches!(
        input.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn parse_knot_type(raw: Option<&str>) -> Result<KnotType, app::AppError> {
    raw.unwrap_or("work")
        .parse::<KnotType>()
        .map_err(|err| app::AppError::InvalidArgument(err.to_string()))
}

#[cfg(not(tarpaulin_include))]
pub(crate) fn run_workflow_command(
    args: &cli::WorkflowArgs,
    repo_root: &std::path::Path,
) -> Result<(), app::AppError> {
    use cli::WorkflowSubcommands;

    match &args.command {
        WorkflowSubcommands::Install(install_args) => run_workflow_install(install_args, repo_root),
        WorkflowSubcommands::Use(use_args) => run_workflow_use(use_args, repo_root),
        WorkflowSubcommands::Current(current_args) => run_workflow_current(current_args, repo_root),
        WorkflowSubcommands::List(list_args) => run_workflow_list(list_args, repo_root),
        WorkflowSubcommands::Show(show_args) => run_workflow_show(show_args, repo_root),
    }
}

#[cfg(not(tarpaulin_include))]
fn run_workflow_install(
    install_args: &cli::WorkflowInstallArgs,
    repo_root: &std::path::Path,
) -> Result<(), app::AppError> {
    let knot_type = install_args
        .knot_type
        .parse::<KnotType>()
        .map_err(|err| app::AppError::InvalidArgument(err.to_string()))?;
    let workflow_id = installed_workflows::install_bundle(repo_root, &install_args.source)?;
    let config = installed_workflows::register_workflow_for_knot_type(
        repo_root,
        knot_type,
        &workflow_id,
        None,
        false,
    )?;
    let set_default = match install_args.set_default.as_deref() {
        Some(raw) => parse_bool_flag(raw)?,
        None => prompt_install_default(&workflow_id)?,
    };
    let config = if set_default {
        installed_workflows::set_current_workflow_selection_for_knot_type(
            repo_root,
            knot_type,
            &workflow_id,
            None,
            None,
        )?
    } else {
        config
    };
    if set_default {
        let profile = config
            .default_profile_id_for_workflow(&workflow_id)
            .unwrap_or_default();
        println!("installed workflow: {workflow_id} (default profile={profile})");
    } else {
        println!("installed workflow: {workflow_id}");
    }
    Ok(())
}

#[cfg(not(tarpaulin_include))]
fn run_workflow_use(
    use_args: &cli::WorkflowUseArgs,
    repo_root: &std::path::Path,
) -> Result<(), app::AppError> {
    let knot_type = parse_knot_type(use_args.knot_type.as_deref())?;
    let config = installed_workflows::set_current_workflow_selection_for_knot_type(
        repo_root,
        knot_type,
        &use_args.id,
        use_args.version,
        use_args.profile.as_deref(),
    )?;
    let workflow_id = config
        .current_workflow_ref_for_knot_type(knot_type)
        .map(|workflow| workflow.workflow_id)
        .unwrap_or_else(|| use_args.id.clone());
    let version = config
        .current_workflow_ref_for_knot_type(knot_type)
        .and_then(|workflow| workflow.version);
    if let Some(version) = version {
        if let Some(profile) = config.default_profile_id_for_workflow(&workflow_id) {
            let profile = profile.rsplit('/').next().unwrap_or(profile);
            println!("default workflow: {workflow_id} v{version} profile={profile}");
        } else {
            println!("default workflow: {workflow_id} v{version}");
        }
    } else {
        println!("default workflow: {workflow_id}");
    }
    Ok(())
}

#[cfg(not(tarpaulin_include))]
fn run_workflow_current(
    current_args: &cli::WorkflowCurrentArgs,
    repo_root: &std::path::Path,
) -> Result<(), app::AppError> {
    let knot_type = parse_knot_type(current_args.knot_type.as_deref())?;
    let registry = installed_workflows::InstalledWorkflowRegistry::load(repo_root)?;
    let workflow = registry.current_workflow_for_knot_type(knot_type)?;
    if current_args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "knot_type": knot_type.as_str(),
                "id": workflow.id,
                "version": workflow.version,
                "builtin": workflow.builtin,
                "bundle_default_profile": workflow.default_profile,
                "default_profile": registry
                    .default_profile_id_for_knot_type(knot_type)
                    .map(|profile| {
                        profile
                            .rsplit('/')
                            .next()
                            .unwrap_or(profile.as_str())
                            .to_string()
                    }),
            }))
            .expect("json serialization should work")
        );
    } else if let Some(profile) = registry.default_profile_id_for_knot_type(knot_type) {
        let profile = profile
            .rsplit('/')
            .next()
            .unwrap_or(profile.as_str())
            .to_string();
        println!(
            "{} v{} default_profile={profile}",
            workflow.id, workflow.version
        );
    } else {
        println!("{} v{}", workflow.id, workflow.version);
    }
    Ok(())
}

#[cfg(not(tarpaulin_include))]
fn run_workflow_list(
    list_args: &cli::WorkflowListArgs,
    repo_root: &std::path::Path,
) -> Result<(), app::AppError> {
    let knot_type = parse_knot_type(list_args.knot_type.as_deref())?;
    let registry = installed_workflows::InstalledWorkflowRegistry::load(repo_root)?;
    let current_id = registry
        .current_workflow_id_for_knot_type(knot_type)
        .to_string();
    let current_version = registry
        .current_workflow_ref_for_knot_type(knot_type)
        .version;
    let workflows = registry
        .registered_workflows_for_knot_type(knot_type)
        .into_iter()
        .map(|workflow| {
            serde_json::json!({
                "knot_type": knot_type.as_str(),
                "id": workflow.id,
                "version": workflow.version,
                "builtin": workflow.builtin,
                "bundle_default_profile": workflow.default_profile,
                "default_profile": registry
                    .default_profile_id_for_workflow(&workflow.id)
                    .map(|profile| {
                        profile
                            .rsplit('/')
                            .next()
                            .unwrap_or(profile.as_str())
                            .to_string()
                    }),
                "current": workflow.id == current_id
                    && Some(workflow.version) == current_version,
                "profiles":
                    workflow.profiles.keys().cloned().collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    if list_args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&workflows).expect("json serialization should work")
        );
    } else if workflows.is_empty() {
        println!("no workflows installed");
    } else {
        for item in workflows {
            print_workflow_list_item(&item);
        }
    }
    Ok(())
}

#[cfg(not(tarpaulin_include))]
fn print_workflow_list_item(item: &serde_json::Value) {
    let id = item["id"].as_str().unwrap_or_default();
    let version = item["version"].as_u64().unwrap_or_default();
    let suffix = if item["current"].as_bool() == Some(true) {
        " (current)"
    } else {
        ""
    };
    let default_profile = item["default_profile"].as_str().unwrap_or_default();
    if default_profile.is_empty() {
        println!("{id} v{version}{suffix}");
    } else {
        println!("{id} v{version}{suffix} default_profile={default_profile}");
    }
}

#[cfg(not(tarpaulin_include))]
fn run_workflow_show(
    show_args: &cli::WorkflowShowArgs,
    repo_root: &std::path::Path,
) -> Result<(), app::AppError> {
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_bool_flag, prompt_install_default};
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    use std::io::Write;

    const PROMPT_HELPER_ENV: &str = "KNOTS_WORKFLOW_PROMPT_HELPER";
    const PROMPT_EXPECT_ENV: &str = "KNOTS_WORKFLOW_PROMPT_EXPECT";

    fn run_prompt_helper(input: &str, expected: bool) {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("pty should open");
        let mut cmd = CommandBuilder::new(
            std::env::current_exe().expect("current test binary should resolve"),
        );
        cmd.arg("--exact");
        cmd.arg("workflow_commands::tests::prompt_install_default_tty_helper");
        cmd.arg("--nocapture");
        cmd.env(PROMPT_HELPER_ENV, "1");
        cmd.env(PROMPT_EXPECT_ENV, if expected { "true" } else { "false" });
        let mut child = pair
            .slave
            .spawn_command(cmd)
            .expect("helper process should spawn");
        drop(pair.slave);

        let mut writer = pair.master.take_writer().expect("writer should open");
        writer
            .write_all(input.as_bytes())
            .expect("helper input should write");
        drop(writer);

        let status = child.wait().expect("helper should exit cleanly");
        assert!(status.success(), "helper process should succeed");
    }

    #[test]
    fn parse_bool_flag_accepts_supported_values() {
        for raw in ["yes", "YES", "true", "1"] {
            assert!(parse_bool_flag(raw).expect("truthy value should parse"));
        }
        for raw in ["no", "NO", "false", "0"] {
            assert!(!parse_bool_flag(raw).expect("falsy value should parse"));
        }
    }

    #[test]
    fn parse_bool_flag_rejects_invalid_values() {
        let err = parse_bool_flag("maybe").expect_err("invalid value should fail");
        assert!(err.to_string().contains("invalid boolean value"));
    }

    #[test]
    fn prompt_install_default_is_disabled_without_tty() {
        assert!(!prompt_install_default("custom_flow").expect("non-interactive prompt should skip"));
    }

    #[test]
    fn prompt_install_default_accepts_yes_from_tty() {
        run_prompt_helper("yes\n", true);
    }

    #[test]
    fn prompt_install_default_rejects_non_yes_from_tty() {
        run_prompt_helper("no\n", false);
    }

    #[test]
    fn prompt_install_default_tty_helper() {
        if std::env::var_os(PROMPT_HELPER_ENV).is_none() {
            return;
        }
        let expected =
            std::env::var(PROMPT_EXPECT_ENV).expect("helper expectation should be set") == "true";
        let result = prompt_install_default("custom_flow").expect("prompt should succeed");
        assert_eq!(result, expected);
    }
}
