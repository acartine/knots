mod app;
mod cli;
mod db;
mod doctor;
mod init;
mod domain;
mod events;
mod fsck;
mod hierarchy_alias;
mod imports;
mod knot_id;
mod list_layout;
mod listing;
mod locks;
mod perf;
mod remote_init;
mod replication;
mod self_manage;
mod snapshots;
mod sync;
mod tiering;
mod ui;
mod workflow;

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {}", err);
        std::process::exit(1);
    }
}

fn run() -> Result<(), app::AppError> {
    use app::UpdateKnotPatch;
    use clap::Parser;
    use cli::{ColdSubcommands, Commands, EdgeSubcommands, ImportSubcommands, WorkflowSubcommands};
    use domain::metadata::MetadataEntryInput;

    let cli = cli::Cli::parse();
    if let Some(outcome) = maybe_run_self_command(&cli.command)? {
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

    let app = app::App::open(&cli.db, cli.repo_root)?;

    match cli.command {
        Commands::New(args) => {
            let knot = app.create_knot(
                &args.title,
                args.body.as_deref(),
                args.state.as_deref(),
                Some(args.workflow.as_str()),
            )?;
            println!(
                "created {} [{}] {}",
                knot_ref(&knot),
                knot.state,
                knot.title
            );
        }
        Commands::State(args) => {
            let knot =
                app.set_state(&args.id, &args.state, args.force, args.if_match.as_deref())?;
            println!("updated {} -> {}", knot_ref(&knot), knot.state);
        }
        Commands::Update(args) => {
            let add_note = args.add_note.map(|content| MetadataEntryInput {
                content,
                username: args.note_username,
                datetime: args.note_datetime,
                agentname: args.note_agentname,
                model: args.note_model,
                version: args.note_version,
            });
            let add_handoff_capsule = args.add_handoff_capsule.map(|content| MetadataEntryInput {
                content,
                username: args.handoff_username,
                datetime: args.handoff_datetime,
                agentname: args.handoff_agentname,
                model: args.handoff_model,
                version: args.handoff_version,
            });
            let patch = UpdateKnotPatch {
                title: args.title,
                description: args.description,
                priority: args.priority,
                status: args.status,
                knot_type: args.knot_type,
                add_tags: args.add_tags,
                remove_tags: args.remove_tags,
                add_note,
                add_handoff_capsule,
                expected_workflow_etag: args.if_match,
                force: args.force,
            };
            let knot = app.update_knot(&args.id, patch)?;
            println!(
                "updated {} [{}] {}",
                knot_ref(&knot),
                knot.state,
                knot.title
            );
        }
        Commands::Ls(args) => {
            let filter = listing::KnotListFilter {
                include_all: args.all,
                state: args.state.clone(),
                knot_type: args.knot_type.clone(),
                workflow_id: args.workflow_id.clone(),
                tags: args.tags.clone(),
                query: args.query.clone(),
            };
            let knots = listing::apply_filters(app.list_knots()?, &filter);
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&knots).expect("json serialization should work")
                );
            } else {
                let layout_edges = app.list_layout_edges()?;
                let rows = list_layout::layout_knots(knots, &layout_edges);
                ui::print_knot_list(&rows, &filter);
            }
        }
        Commands::Show(args) => match app.show_knot(&args.id)? {
            Some(knot) => {
                if args.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&knot)
                            .expect("json serialization should work")
                    );
                } else {
                    ui::print_knot_show(&knot);
                }
            }
            None => return Err(app::AppError::NotFound(args.id)),
        },
        Commands::Workflow(args) => match args.command {
            WorkflowSubcommands::List(list_args) => {
                let workflows = app.list_workflows();
                if list_args.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&workflows)
                            .expect("json serialization should work")
                    );
                } else if workflows.is_empty() {
                    println!("no workflows found");
                } else {
                    for workflow in workflows {
                        println!(
                            "{} initial={} terminal={}",
                            workflow.id,
                            workflow.initial_state,
                            workflow.terminal_states.join(",")
                        );
                    }
                }
            }
            WorkflowSubcommands::Show(show_args) => {
                let workflow = app.show_workflow(&show_args.id)?;
                if show_args.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&workflow)
                            .expect("json serialization should work")
                    );
                } else {
                    println!("id: {}", workflow.id);
                    if let Some(description) = workflow.description.as_deref() {
                        println!("description: {}", description);
                    }
                    println!("initial_state: {}", workflow.initial_state);
                    println!("states: {}", workflow.states.join(", "));
                    println!("terminal_states: {}", workflow.terminal_states.join(", "));
                    println!("transitions:");
                    for transition in workflow.transitions {
                        println!("  {} -> {}", transition.from, transition.to);
                    }
                }
            }
        },
        Commands::Pull(args) => {
            let summary = app.pull()?;
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&summary).expect("json serialization should work")
                );
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
        }
        Commands::Push(args) => {
            let summary = app.push()?;
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&summary).expect("json serialization should work")
                );
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
                println!(
                    "{}",
                    serde_json::to_string_pretty(&summary).expect("json serialization should work")
                );
            } else {
                println!(
                    "sync push(local_event_files={} copied_files={} committed={} pushed={}) \
                     pull(head={} index_files={} full_files={} knot_updates={} edge_adds={} edge_removes={})",
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
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).expect("json serialization should work")
                );
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
            let report = app.doctor()?;
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).expect("json serialization should work")
                );
            } else {
                for check in &report.checks {
                    println!(
                        "{} [{}] {}",
                        check.name,
                        serde_json::to_string(&check.status)
                            .expect("status serialization should work")
                            .trim_matches('"'),
                        check.detail
                    );
                }
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
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).expect("json serialization should work")
                );
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
                println!(
                    "{}",
                    serde_json::to_string_pretty(&summary).expect("json serialization should work")
                );
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
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&summary)
                            .expect("json serialization should work")
                    );
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
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&matches)
                            .expect("json serialization should work")
                    );
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
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&knot)
                            .expect("json serialization should work")
                    );
                } else {
                    println!(
                        "rehydrated {} [{}] {}",
                        knot_ref(&knot),
                        knot.state,
                        knot.title
                    );
                }
            }
            None => return Err(app::AppError::NotFound(args.id)),
        },
        Commands::Edge(args) => match args.command {
            EdgeSubcommands::Add(edge_args) => {
                let edge = app.add_edge(&edge_args.src, &edge_args.kind, &edge_args.dst)?;
                println!("edge added: {} -[{}]-> {}", edge.src, edge.kind, edge.dst);
            }
            EdgeSubcommands::Remove(edge_args) => {
                let edge = app.remove_edge(&edge_args.src, &edge_args.kind, &edge_args.dst)?;
                println!("edge removed: {} -[{}]-> {}", edge.src, edge.kind, edge.dst);
            }
            EdgeSubcommands::List(edge_args) => {
                let edges = app.list_edges(&edge_args.id, &edge_args.direction)?;
                if edge_args.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&edges)
                            .expect("json serialization should work")
                    );
                } else if edges.is_empty() {
                    println!("no edges for {}", edge_args.id);
                } else {
                    for edge in edges {
                        println!("{} -[{}]-> {}", edge.src, edge.kind, edge.dst);
                    }
                }
            }
        },
        Commands::Import(args) => match args.command {
            ImportSubcommands::Jsonl(import_args) => {
                let summary = app.import_jsonl(
                    &import_args.file,
                    import_args.since.as_deref(),
                    import_args.dry_run,
                )?;
                println!(
                    "import {} {}: status={}, processed={}, imported={}, skipped={}, errors={}",
                    summary.source_type,
                    summary.source_ref,
                    summary.status,
                    summary.processed_count,
                    summary.imported_count,
                    summary.skipped_count,
                    summary.error_count
                );
                if let Some(checkpoint) = summary.checkpoint {
                    println!("checkpoint: {}", checkpoint);
                }
                if let Some(error) = summary.last_error {
                    println!("last_error: {}", error);
                }
            }
            ImportSubcommands::Dolt(import_args) => {
                let summary = app.import_dolt(
                    &import_args.repo,
                    import_args.since.as_deref(),
                    import_args.dry_run,
                )?;
                println!(
                    "import {} {}: status={}, processed={}, imported={}, skipped={}, errors={}",
                    summary.source_type,
                    summary.source_ref,
                    summary.status,
                    summary.processed_count,
                    summary.imported_count,
                    summary.skipped_count,
                    summary.error_count
                );
                if let Some(checkpoint) = summary.checkpoint {
                    println!("checkpoint: {}", checkpoint);
                }
                if let Some(error) = summary.last_error {
                    println!("last_error: {}", error);
                }
            }
            ImportSubcommands::Status(import_args) => {
                let statuses = app.import_statuses()?;
                if import_args.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&statuses)
                            .expect("json serialization should work")
                    );
                } else if statuses.is_empty() {
                    println!("no import runs found");
                } else {
                    for status in statuses {
                        println!(
                            "{} {} status={} processed={} imported={} skipped={} errors={}",
                            status.source_type,
                            status.source_ref,
                            status.status,
                            status.processed_count,
                            status.imported_count,
                            status.skipped_count,
                            status.error_count
                        );
                        if let Some(checkpoint) = status.checkpoint {
                            println!("  checkpoint={}", checkpoint);
                        }
                        if let Some(error) = status.last_error {
                            println!("  last_error={}", error);
                        }
                    }
                }
            }
        },
        Commands::Upgrade(_) => unreachable!("self management commands return before app init"),
        Commands::Uninstall(_) => unreachable!("self management commands return before app init"),
        Commands::SelfManage(_) => unreachable!("self management commands return before app init"),
    }

    Ok(())
}

fn knot_ref(knot: &app::KnotView) -> String {
    match knot.alias.as_deref() {
        Some(alias) => format!("{alias} ({})", knot.id),
        None => knot.id.clone(),
    }
}

fn maybe_run_self_command(command: &cli::Commands) -> Result<Option<String>, app::AppError> {
    use cli::Commands;

    match command {
        Commands::Upgrade(update_args) => {
            self_manage::run_update(&self_manage::SelfUpdateOptions {
                version: update_args.version.clone(),
                repo: update_args.repo.clone(),
                install_dir: update_args.install_dir.clone(),
                script_url: update_args.script_url.clone(),
            })?;
            Ok(Some("updated kno binary".to_string()))
        }
        Commands::Uninstall(uninstall_args) => {
            let result = self_manage::run_uninstall(&self_manage::SelfUninstallOptions {
                bin_path: uninstall_args.bin_path.clone(),
                remove_previous: uninstall_args.remove_previous,
            })?;
            let mut lines = vec![format!("removed {}", result.binary_path.display())];
            if result.removed_previous {
                lines.push("removed previous backups (kno.previous/knots.previous)".to_string());
            }
            Ok(Some(lines.join("\n")))
        }
        Commands::SelfManage(args) => match &args.command {
            cli::SelfSubcommands::Update(update_args) => {
                self_manage::run_update(&self_manage::SelfUpdateOptions {
                    version: update_args.version.clone(),
                    repo: update_args.repo.clone(),
                    install_dir: update_args.install_dir.clone(),
                    script_url: update_args.script_url.clone(),
                })?;
                Ok(Some("updated kno binary".to_string()))
            }
            cli::SelfSubcommands::Uninstall(uninstall_args) => {
                let result = self_manage::run_uninstall(&self_manage::SelfUninstallOptions {
                    bin_path: uninstall_args.bin_path.clone(),
                    remove_previous: uninstall_args.remove_previous,
                })?;
                let mut lines = vec![format!("removed {}", result.binary_path.display())];
                if result.removed_previous {
                    lines
                        .push("removed previous backups (kno.previous/knots.previous)".to_string());
                }
                Ok(Some(lines.join("\n")))
            }
        },
        _ => Ok(None),
    }
}
