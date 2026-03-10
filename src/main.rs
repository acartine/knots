mod app;
mod cli;
mod cli_help;
mod cli_ops;
mod completions;
mod db;
mod dispatch;
mod doctor;
mod doctor_fix;
mod domain;
mod events;
mod fsck;
mod git_hooks;
mod hierarchy_alias;
mod init;
mod knot_id;
mod list_layout;
#[cfg(test)]
mod list_layout_tests_ext;
mod listing;
mod locks;
#[cfg(test)]
mod main_tests;
mod perf;
mod poll_claim;
mod profile;
mod profile_commands;
mod prompt;
mod remote_init;
mod replication;
mod rollback;
mod self_manage;
mod skills;
mod snapshots;
mod state_hierarchy;
mod sync;
mod tiering;
mod ui;
mod workflow;
mod workflow_diagram;
mod write_dispatch;
mod write_queue;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if cli_help::is_toplevel_help(&args) {
        cli_help::print_custom_help();
        return;
    }
    if let Err(err) = run() {
        eprintln!("error: {}", err);
        std::process::exit(1);
    }
}

fn print_json(val: &impl serde::Serialize) {
    let s = serde_json::to_string_pretty(val).expect("json serialize");
    println!("{s}");
}

fn run() -> Result<(), app::AppError> {
    use clap::FromArgMatches;
    use cli::{ColdSubcommands, Commands, EdgeSubcommands};

    let cli = cli::Cli::from_arg_matches_mut(&mut cli::styled_command().get_matches())
        .expect("arg matches should be valid");
    if let Some(outcome) = self_manage::maybe_run_self_command(&cli.command)? {
        println!("{outcome}");
        return Ok(());
    }

    if let Commands::Init = &cli.command {
        init::init_all(&cli.repo_root, &cli.db)?;
        println!("kno init completed");
        return Ok(());
    }
    if let Commands::Uninit = &cli.command {
        init::uninit_all(&cli.repo_root, &cli.db)?;
        println!("kno uninit completed");
        return Ok(());
    }
    if let Commands::Hooks(args) = &cli.command {
        return run_hooks_command(&cli.repo_root, &args.command);
    }
    if let Commands::Completions(args) = &cli.command {
        return completions::run_completions_command(args.shell.as_deref(), args.install);
    }
    if let Commands::Profile(args) = &cli.command {
        return profile_commands::run_profile_command(args, &cli.repo_root, &cli.db);
    }
    if let Some(output) = write_dispatch::maybe_run_queued_command(&cli)? {
        print!("{output}");
        return Ok(());
    }

    let app = app::App::open(&cli.db, cli.repo_root)?;

    match cli.command {
        Commands::New(_) => {
            unreachable!("queued write commands are handled before app initialization")
        }
        Commands::State(_) => {
            unreachable!("queued write commands are handled before app initialization")
        }
        Commands::Update(_) => {
            unreachable!("queued write commands are handled before app initialization")
        }
        Commands::Ls(args) => {
            let filter = listing::KnotListFilter {
                include_all: args.all,
                state: args.state.clone(),
                knot_type: args.knot_type.clone(),
                profile_id: args.profile_id.clone(),
                tags: args.tags.clone(),
                query: args.query.clone(),
            };
            let knots = listing::apply_filters(app.list_knots()?, &filter);
            if args.json {
                print_json(&knots);
            } else {
                let layout_edges = app.list_layout_edges()?;
                let rows = list_layout::layout_knots(knots, &layout_edges);
                ui::print_knot_list(&rows, &filter);
            }
        }
        Commands::Show(args) => match app.show_knot(&args.id)? {
            Some(knot) => {
                if args.json {
                    let mut value = serde_json::to_value(&knot).expect("json serialize");
                    if !args.verbose {
                        ui::trim_json_metadata(&mut value, &knot);
                    }
                    print_json(&value);
                } else {
                    ui::print_knot_show(&knot, args.verbose);
                }
            }
            None => return Err(app::AppError::NotFound(args.id)),
        },
        Commands::Profile(_) => {
            unreachable!("profile commands are handled before app initialization")
        }
        Commands::Rollback(_) => {
            unreachable!("queued write commands are handled before app initialization")
        }
        Commands::Pull(args) => {
            let summary = app.pull()?;
            let drift_warning = app.pull_drift_warning()?;
            if args.json {
                print_json(&summary);
            } else {
                println!(
                    concat!(
                        "pull head={} index_files={} full_files={} ",
                        "knot_updates={} edge_adds={} edge_removes={}"
                    ),
                    summary.target_head,
                    summary.index_files,
                    summary.full_files,
                    summary.knot_updates,
                    summary.edge_adds,
                    summary.edge_removes
                );
            }
            if let Some(warning) = drift_warning {
                eprintln!(
                    "warning: local knots drift is high (unpushed_event_files={} > \
                     threshold={}); run `kno push`",
                    warning.unpushed_event_files, warning.threshold
                );
            }
        }
        Commands::Push(args) => {
            let summary = app.push()?;
            if args.json {
                print_json(&summary);
            } else {
                println!(
                    "push local_event_files={} copied_files={} committed={} pushed={}{}",
                    summary.local_event_files,
                    summary.copied_files,
                    summary.committed,
                    summary.pushed,
                    summary
                        .commit
                        .as_ref()
                        .map(|commit| format!(" commit={commit}"))
                        .unwrap_or_default()
                );
            }
        }
        Commands::Sync(args) => {
            let summary = app.sync()?;
            if args.json {
                print_json(&summary);
            } else {
                println!(
                    "sync push(local_event_files={} copied_files={} committed={} pushed={}) \
                     pull(head={} index_files={} full_files={} knot_updates={} \
                     edge_adds={} edge_removes={})",
                    summary.push.local_event_files,
                    summary.push.copied_files,
                    summary.push.committed,
                    summary.push.pushed,
                    summary.pull.target_head,
                    summary.pull.index_files,
                    summary.pull.full_files,
                    summary.pull.knot_updates,
                    summary.pull.edge_adds,
                    summary.pull.edge_removes
                );
            }
        }
        Commands::Init => unreachable!("init is handled before app initialization"),
        Commands::Uninit => unreachable!("uninit is handled before app initialization"),
        Commands::InitRemote => {
            app.init_remote()?;
            println!("initialized remote branch origin/knots");
        }
        Commands::Fsck(args) => {
            let report = app.fsck()?;
            if args.json {
                print_json(&report);
            } else {
                println!(
                    "fsck scanned_files={} issues={}",
                    report.files_scanned,
                    report.issues.len()
                );
                for issue in &report.issues {
                    println!("  - {}: {}", issue.path, issue.message);
                }
            }
            if !report.ok() {
                return Err(app::AppError::InvalidArgument(format!(
                    "fsck found {} issue(s)",
                    report.issues.len()
                )));
            }
        }
        Commands::Doctor(args) => {
            let report = app.doctor(args.fix)?;
            if args.json {
                print_json(&report);
            } else {
                ui::print_doctor_report(&report);
            }
            if !args.fix && doctor_fix::has_non_pass_checks(&report.checks) {
                eprintln!("kno doctor --fix to address these items");
            }
            if report.failure_count() > 0 {
                return Err(app::AppError::InvalidArgument(format!(
                    "doctor found {} failing check(s)",
                    report.failure_count()
                )));
            }
        }
        Commands::Perf(args) => {
            let report = app.perf_harness(args.iterations)?;
            if args.json {
                print_json(&report);
            } else {
                println!("perf iterations={}", report.iterations);
                for measurement in &report.measurements {
                    println!(
                        "  {} elapsed_ms={:.2} budget_ms={:.2} within_budget={}",
                        measurement.name,
                        measurement.elapsed_ms,
                        measurement.budget_ms,
                        measurement.within_budget
                    );
                }
            }
            if args.strict && report.over_budget_count() > 0 {
                return Err(app::AppError::InvalidArgument(format!(
                    "perf regression: {} measurement(s) over budget",
                    report.over_budget_count()
                )));
            }
        }
        Commands::Compact(args) => {
            if !args.write_snapshots {
                return Err(app::AppError::InvalidArgument(
                    "compact currently requires --write-snapshots".to_string(),
                ));
            }
            let summary = app.compact_write_snapshots()?;
            if args.json {
                print_json(&summary);
            } else {
                println!(
                    "snapshots written hot={} warm={} cold={} active={} cold_path={}",
                    summary.hot_count,
                    summary.warm_count,
                    summary.cold_count,
                    summary.active_path.display(),
                    summary.cold_path.display()
                );
            }
        }
        Commands::Cold(args) => match args.command {
            ColdSubcommands::Sync(sync_args) => {
                let summary = app.cold_sync()?;
                if sync_args.json {
                    print_json(&summary);
                } else {
                    println!(
                        concat!(
                            "cold sync head={} index_files={} full_files={} ",
                            "knot_updates={} edge_adds={} edge_removes={}"
                        ),
                        summary.target_head,
                        summary.index_files,
                        summary.full_files,
                        summary.knot_updates,
                        summary.edge_adds,
                        summary.edge_removes
                    );
                }
            }
            ColdSubcommands::Search(search_args) => {
                let matches = app.cold_search(&search_args.term)?;
                if search_args.json {
                    print_json(&matches);
                } else if matches.is_empty() {
                    println!("no cold knots matched '{}'", search_args.term);
                } else {
                    for knot in matches {
                        println!(
                            "{} [{}] {} ({})",
                            knot.id, knot.state, knot.title, knot.updated_at
                        );
                    }
                }
            }
        },
        Commands::Rehydrate(args) => match app.rehydrate(&args.id)? {
            Some(knot) => {
                if args.json {
                    print_json(&knot);
                } else {
                    println!(
                        "rehydrated {} [{}] {}",
                        dispatch::knot_ref(&knot),
                        knot.state,
                        knot.title
                    );
                }
            }
            None => return Err(app::AppError::NotFound(args.id)),
        },
        Commands::Edge(args) => match args.command {
            EdgeSubcommands::Add(_) => {
                unreachable!("queued write commands are handled before app initialization")
            }
            EdgeSubcommands::Remove(_) => {
                unreachable!("queued write commands are handled before app initialization")
            }
            EdgeSubcommands::List(edge_args) => {
                let edges = app.list_edges(&edge_args.id, &edge_args.direction)?;
                if edge_args.json {
                    print_json(&edges);
                } else if edges.is_empty() {
                    println!("no edges for {}", edge_args.id);
                } else {
                    for edge in edges {
                        println!("{} -[{}]-> {}", edge.src, edge.kind, edge.dst);
                    }
                }
            }
        },
        Commands::Next(_) => {
            unreachable!("queued write commands are handled before app initialization")
        }
        Commands::Skill(args) => {
            let content = match dispatch::resolve_next_state(&app, &args.id) {
                Ok((_knot, next, _owner)) => skills::skill_for_state(&next),
                Err(app::AppError::NotFound(_)) => {
                    let normalized = args.id.trim().to_ascii_lowercase().replace('-', "_");
                    skills::skill_for_state(&normalized)
                }
                Err(err) => return Err(err),
            };
            let content = content.ok_or_else(|| {
                app::AppError::InvalidArgument(format!(
                    "'{}' is not a knot id or skill state name",
                    args.id
                ))
            })?;
            print!("{content}");
        }
        Commands::Q(_) => {
            unreachable!("queued write commands are handled before app initialization")
        }
        Commands::Poll(args) => {
            if args.claim {
                unreachable!("queued write commands are handled before app initialization");
            }
            poll_claim::run_poll(&app, args)?;
        }
        Commands::Claim(args) => {
            if !args.peek {
                unreachable!("queued write commands are handled before app initialization");
            }
            poll_claim::run_claim(&app, args)?;
        }
        Commands::Ready(args) => poll_claim::run_ready(&app, args)?,
        Commands::Step(_) => {
            unreachable!("queued write commands are handled before app initialization")
        }
        Commands::Upgrade(_) => unreachable!("self management commands return before app init"),
        Commands::Uninstall(_) => unreachable!("self management commands return before app init"),
        Commands::Completions(_) => unreachable!("completions handled before app init"),
        Commands::Hooks(_) => unreachable!("hooks handled before app init"),
    }

    Ok(())
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
