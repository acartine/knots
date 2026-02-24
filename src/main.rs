mod app;
mod cli;
mod db;
mod domain;
mod events;
mod imports;
mod listing;
mod self_manage;
mod sync;
mod ui;

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {}", err);
        std::process::exit(1);
    }
}

fn run() -> Result<(), app::AppError> {
    use app::UpdateKnotPatch;
    use clap::Parser;
    use cli::{Commands, EdgeSubcommands, ImportSubcommands};
    use domain::metadata::MetadataEntryInput;

    let cli = cli::Cli::parse();
    if let Some(outcome) = maybe_run_self_command(&cli.command)? {
        println!("{outcome}");
        return Ok(());
    }

    let app = app::App::open(&cli.db, cli.repo_root)?;

    match cli.command {
        Commands::New(args) => {
            let knot = app.create_knot(&args.title, args.body.as_deref(), &args.state)?;
            println!("created {} [{}] {}", knot.id, knot.state, knot.title);
        }
        Commands::State(args) => {
            let knot = app.set_state(&args.id, &args.state, args.force)?;
            println!("updated {} -> {}", knot.id, knot.state);
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
                force: args.force,
            };
            let knot = app.update_knot(&args.id, patch)?;
            println!("updated {} [{}] {}", knot.id, knot.state, knot.title);
        }
        Commands::Ls(args) => {
            let filter = listing::KnotListFilter {
                state: args.state.clone(),
                knot_type: args.knot_type.clone(),
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
                ui::print_knot_list(&knots, &filter);
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
                    println!("id: {}", knot.id);
                    println!("title: {}", knot.title);
                    println!("state: {}", knot.state);
                    println!("updated_at: {}", knot.updated_at);
                    if let Some(created_at) = knot.created_at {
                        println!("created_at: {}", created_at);
                    }
                    if let Some(body) = knot.body {
                        println!("body: {}", body);
                    }
                    if let Some(description) = knot.description {
                        println!("description: {}", description);
                    }
                    if let Some(priority) = knot.priority {
                        println!("priority: {}", priority);
                    }
                    if let Some(knot_type) = knot.knot_type {
                        println!("type: {}", knot_type);
                    }
                    if !knot.tags.is_empty() {
                        println!("tags: {}", knot.tags.join(", "));
                    }
                    if !knot.notes.is_empty() {
                        println!("notes: {}", knot.notes.len());
                    }
                    if !knot.handoff_capsules.is_empty() {
                        println!("handoff_capsules: {}", knot.handoff_capsules.len());
                    }
                }
            }
            None => return Err(app::AppError::NotFound(args.id)),
        },
        Commands::Sync(args) => {
            let summary = app.sync()?;
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&summary).expect("json serialization should work")
                );
            } else {
                println!(
                    concat!(
                        "sync head={} index_files={} full_files={} ",
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
