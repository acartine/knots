mod app;
mod cli;
mod db;
mod domain;
mod events;
mod imports;
mod sync;

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {}", err);
        std::process::exit(1);
    }
}

fn run() -> Result<(), app::AppError> {
    use clap::Parser;
    use cli::{Commands, EdgeSubcommands, ImportSubcommands};

    let cli = cli::Cli::parse();
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
        Commands::Ls(args) => {
            let knots = app.list_knots()?;
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&knots).expect("json serialization should work")
                );
            } else {
                for knot in knots {
                    println!("{} [{}] {}", knot.id, knot.state, knot.title);
                }
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
    }

    Ok(())
}
