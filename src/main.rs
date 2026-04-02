mod action_prompt;
mod app;
mod artifact_target;
#[cfg(test)]
mod artifact_target_tests;
mod cli;
mod cli_agent;
mod cli_help;
mod cli_loom;
mod cli_ops;
mod cli_skills;
mod cli_workflow;
mod completions;
mod db;
mod dispatch;
mod doctor;
mod doctor_fix;
mod domain;
mod events;
mod fsck;
mod git_hooks;
#[cfg(test)]
mod git_hooks_tests;
mod hierarchy_alias;
mod init;
mod installed_workflows;
mod knot_id;
mod lease;
mod list_layout;
#[cfg(test)]
mod list_layout_tests;
#[cfg(test)]
mod list_layout_tests_ext;
mod listing;
mod locks;
mod loom_compat_bundle;
mod loom_compat_commands;
mod loom_compat_harness;
#[cfg(test)]
mod loom_compat_harness_tests;
#[cfg(test)]
mod loom_compat_prompt_resolution_tests;
#[cfg(test)]
mod main_tests;
mod managed_skills;
mod perf;
mod poll_claim;
mod profile;
mod profile_commands;
mod profile_normalize;
mod progress;
mod project;
mod project_commands;
#[cfg(test)]
mod project_commands_tests;
#[cfg(test)]
mod project_tests_ext;
mod prompt;
#[cfg(test)]
mod prompt_tests;
mod release_version;
mod remote_init;
mod replication;
mod rollback;
mod run_commands;
mod self_manage;
#[cfg(test)]
mod skills;
mod snapshots;
mod state_hierarchy;
mod sync;
mod tiering;
mod ui;
mod upgrade_notice;
mod workflow;
mod workflow_commands;
mod workflow_diagram;
mod workflow_runtime;
mod write_dispatch;
mod write_queue;

fn main() {
    upgrade_notice::maybe_print_upgrade_notice();
    let args: Vec<String> = std::env::args().collect();
    if cli_help::is_toplevel_help(&args) {
        cli_help::print_custom_help();
        return;
    }
    if let Err(err) = run() {
        eprint!("{}", format_error(&err));
        std::process::exit(1);
    }
}

fn format_error(err: &app::AppError) -> String {
    let mut msg = format!("error: {}\n", err);
    if matches!(err, app::AppError::NotFound(_)) {
        msg.push_str(
            "hint: if running in a git worktree, \
             try: kno -C <repo_root> ...\n",
        );
    }
    msg
}

pub(crate) fn print_json(val: &impl serde::Serialize) {
    let s = serde_json::to_string_pretty(val).expect("json serialize");
    println!("{s}");
}

fn resolve_db_path(context: &project::ProjectContext, db_path: Option<&str>) -> String {
    let default_path = context.store_paths.db_path();
    let Some(db_path) = db_path else {
        return default_path.display().to_string();
    };
    let db = std::path::Path::new(db_path);
    if db.is_absolute() {
        return db.display().to_string();
    }
    match context.distribution {
        project::DistributionMode::Git => context.repo_root.join(db).display().to_string(),
        project::DistributionMode::LocalOnly => {
            context.store_paths.root.join(db).display().to_string()
        }
    }
}

pub(crate) fn progress_reporter(enabled: bool) -> Option<ui::StdoutProgressReporter> {
    enabled.then(ui::StdoutProgressReporter::auto)
}

fn run() -> Result<(), app::AppError> {
    use clap::FromArgMatches;
    use cli::Commands;

    let cli = cli::Cli::from_arg_matches_mut(&mut cli::styled_command().get_matches())
        .expect("arg matches should be valid");
    let cwd = std::env::current_dir()?;
    let explicit_repo_root = cli.repo_root.as_deref();

    if let Some(outcome) = self_manage::maybe_run_self_command(&cli.command, &cwd)? {
        println!("{outcome}");
        return Ok(());
    }

    if let Commands::Project(args) = &cli.command {
        return project_commands::run_project_command(args, None, explicit_repo_root);
    }

    if let Commands::Init = &cli.command {
        if let Some(project_id) = cli.project.as_deref() {
            let repo_root = explicit_repo_root.or(Some(cwd.as_path()));
            let _ = project::load_named_project(None, project_id)
                .or_else(|_| project::create_named_project(None, project_id, repo_root))
                .map_err(app::AppError::InvalidArgument)?;
            project::set_active_project(None, project_id)
                .map_err(app::AppError::InvalidArgument)?;
            let context = project::resolve_context(Some(project_id), None, &cwd, None)
                .map_err(app::AppError::InvalidArgument)?;
            let db_path = resolve_db_path(&context, cli.db.as_deref());
            init::init_local_store(&context.repo_root, &db_path)?;
            println!("kno init completed");
            return Ok(());
        }
        let context = project::resolve_context(None, explicit_repo_root, &cwd, None)
            .map_err(app::AppError::InvalidArgument)?;
        let db_path = resolve_db_path(&context, cli.db.as_deref());
        init::init_all(&context.repo_root, &db_path)?;
        println!("kno init completed");
        return Ok(());
    }
    let context = project::resolve_context(cli.project.as_deref(), explicit_repo_root, &cwd, None)
        .map_err(app::AppError::InvalidArgument)?;
    let db_path = resolve_db_path(&context, cli.db.as_deref());

    if let Commands::Uninit = &cli.command {
        match context.distribution {
            project::DistributionMode::Git => init::uninit_all(&context.repo_root, &db_path)?,
            project::DistributionMode::LocalOnly => {
                init::uninit_local_store(&context.repo_root, &db_path)?;
                if let Some(project_id) = context.project_id.as_deref() {
                    let _ = project::clear_active_project(None);
                    println!("removed local store for project {}", project_id);
                    return Ok(());
                }
            }
        }
        println!("kno uninit completed");
        return Ok(());
    }
    if let Commands::Hooks(args) = &cli.command {
        if context.distribution != project::DistributionMode::Git {
            return Err(app::AppError::UnsupportedDistribution {
                action: "hooks".to_string(),
                mode: "local-only".to_string(),
            });
        }
        return run_hooks_command(&context.repo_root, &args.command);
    }
    if let Commands::Completions(args) = &cli.command {
        return completions::run_completions_command(args.shell.as_deref(), args.install);
    }
    if let Commands::Skills(args) = &cli.command {
        return run_skills_command(&context.repo_root, args);
    }
    if let Commands::Profile(args) = &cli.command {
        return profile_commands::run_profile_command_with_context(args, &context, &db_path);
    }
    if let Commands::Workflow(args) = &cli.command {
        return workflow_commands::run_workflow_command(args, &context.repo_root);
    }
    if let Commands::Loom(args) = &cli.command {
        return loom_compat_commands::run_loom_command(args, &context.repo_root);
    }
    if let Some(output) =
        write_dispatch::maybe_run_queued_command_with_context(&cli, &context, &db_path)?
    {
        print!("{output}");
        return Ok(());
    }

    let app = app::App::open_with_context(&context, &db_path)?;
    dispatch_read_command(cli.command, &app)
}

fn dispatch_read_command(command: cli::Commands, app: &app::App) -> Result<(), app::AppError> {
    use cli::{Commands, EdgeSubcommands};
    match command {
        Commands::Ls(args) => run_commands::run_ls(app, args),
        Commands::Show(args) => run_commands::run_show(app, args),
        Commands::Pull(args) => run_commands::run_pull(app, args),
        Commands::Push(args) => run_commands::run_push(app, args),
        Commands::Sync(args) => run_commands::run_sync(app, args),
        Commands::InitRemote => {
            app.init_remote()?;
            println!("initialized remote branch origin/knots");
            Ok(())
        }
        Commands::Fsck(args) => run_commands::run_fsck(app, args),
        Commands::Doctor(args) => run_commands::run_doctor(app, args),
        Commands::Perf(args) => run_commands::run_perf(app, args),
        Commands::Compact(args) => run_commands::run_compact(app, args),
        Commands::Cold(args) => run_commands::run_cold(app, args),
        Commands::Rehydrate(args) => run_commands::run_rehydrate(app, args),
        Commands::Edge(args) => match args.command {
            EdgeSubcommands::List(edge_args) => run_commands::run_edge_list(app, edge_args),
            _ => unreachable!("queued write commands handled before app init"),
        },
        Commands::Skill(args) => run_commands::run_skill(app, args),
        Commands::Poll(args) => {
            if args.claim {
                unreachable!("queued write commands handled before app init");
            }
            poll_claim::run_poll(app, args)
        }
        Commands::Claim(args) => {
            if !args.peek {
                unreachable!("queued write commands handled before app init");
            }
            poll_claim::run_claim(app, args)
        }
        Commands::Ready(args) => poll_claim::run_ready(app, args),
        Commands::Lease(args) => run_commands::run_lease_read(app, args),
        _ => unreachable!("handled before app initialization"),
    }
}

fn run_skills_command(
    repo_root: &std::path::Path,
    args: &cli::SkillsArgs,
) -> Result<(), app::AppError> {
    use cli::SkillsSubcommands;
    use managed_skills::SkillsCommand;

    let tool = match &args.command {
        SkillsSubcommands::Install(inner) => target_from_arg(inner.target),
        SkillsSubcommands::Uninstall(inner) => target_from_arg(inner.target),
        SkillsSubcommands::Update(inner) => target_from_arg(inner.target),
    };
    let command = match &args.command {
        SkillsSubcommands::Install(_) => SkillsCommand::Install(tool),
        SkillsSubcommands::Uninstall(_) => SkillsCommand::Uninstall(tool),
        SkillsSubcommands::Update(_) => SkillsCommand::Update(tool),
    };
    let output = managed_skills::run_command(repo_root, command)?;
    println!("{output}");
    Ok(())
}

fn target_from_arg(target: cli_skills::SkillTargetArg) -> managed_skills::SkillTool {
    match target {
        cli_skills::SkillTargetArg::Codex => managed_skills::SkillTool::Codex,
        cli_skills::SkillTargetArg::Claude => managed_skills::SkillTool::Claude,
        cli_skills::SkillTargetArg::OpenCode => managed_skills::SkillTool::OpenCode,
    }
}

fn run_hooks_command(
    repo_root: &std::path::Path,
    command: &cli::HooksSubcommands,
) -> Result<(), app::AppError> {
    use cli::HooksSubcommands;
    match command {
        HooksSubcommands::Install => {
            let summary = git_hooks::install_hooks(repo_root)?;
            for (name, outcome) in &summary.outcomes {
                let label = match outcome {
                    git_hooks::HookInstallOutcome::Installed => "installed",
                    git_hooks::HookInstallOutcome::AlreadyManaged => "up to date",
                    git_hooks::HookInstallOutcome::PreservedExisting => {
                        "installed (existing hook preserved as .local)"
                    }
                };
                println!("{name}: {label}");
            }
        }
        HooksSubcommands::Uninstall => {
            let summary = git_hooks::uninstall_hooks(repo_root)?;
            for (name, outcome) in &summary.outcomes {
                let label = match outcome {
                    git_hooks::HookInstallOutcome::Installed => "removed",
                    _ => "not installed",
                };
                println!("{name}: {label}");
            }
        }
        HooksSubcommands::Status => {
            let report = git_hooks::hooks_status(repo_root);
            for (name, managed) in &report.hooks {
                let label = if *managed { "installed" } else { "missing" };
                println!("{name}: {label}");
            }
        }
    }
    Ok(())
}
