mod app;
mod cli;
mod db;
mod doctor;
mod domain;
mod events;
mod fsck;
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
mod profile;
mod remote_init;
mod replication;
mod self_manage;
mod snapshots;
mod sync;
mod tiering;
mod ui;
mod workflow;
mod workflow_diagram;

use std::io::IsTerminal;

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {}", err);
        std::process::exit(1);
    }
}

fn run() -> Result<(), app::AppError> {
    use app::UpdateKnotPatch;
    use clap::Parser;
    use cli::{ColdSubcommands, Commands, EdgeSubcommands};
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
    if let Commands::Profile(args) = &cli.command {
        return run_profile_command(args, &cli.repo_root, &cli.db);
    }

    let app = app::App::open(&cli.db, cli.repo_root)?;

    match cli.command {
        Commands::New(args) => {
            let knot = app.create_knot(
                &args.title,
                args.body.as_deref(),
                args.state.as_deref(),
                args.profile.as_deref(),
            )?;
            let palette = ui::Palette::auto();
            println!(
                "created {} {} {}",
                palette.id(&knot_ref(&knot)),
                palette.state(&knot.state),
                knot.title
            );
        }
        Commands::State(args) => {
            let knot = app.set_state_with_actor(
                &args.id,
                &args.state,
                args.force,
                args.if_match.as_deref(),
                app::StateActorMetadata {
                    actor_kind: args.actor_kind.clone(),
                    agent_name: args.agent_name.clone(),
                    agent_model: args.agent_model.clone(),
                    agent_version: args.agent_version.clone(),
                },
            )?;
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
                expected_profile_etag: args.if_match,
                force: args.force,
                state_actor: app::StateActorMetadata {
                    actor_kind: args.actor_kind,
                    agent_name: args.agent_name,
                    agent_model: args.agent_model,
                    agent_version: args.agent_version,
                },
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
                profile_id: args.profile_id.clone(),
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
        Commands::Profile(_) => {
            unreachable!("profile commands are handled before app initialization")
        }
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
            let report = app.doctor(args.fix)?;
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
        Commands::Upgrade(_) => unreachable!("self management commands return before app init"),
        Commands::Uninstall(_) => unreachable!("self management commands return before app init"),
    }

    Ok(())
}

fn run_profile_command(
    args: &cli::ProfileArgs,
    repo_root: &std::path::Path,
    db_path: &str,
) -> Result<(), app::AppError> {
    use cli::ProfileSubcommands;

    let registry = workflow::ProfileRegistry::load()?;
    let palette = ProfilePalette::auto();
    match &args.command {
        ProfileSubcommands::List(list_args) => {
            let profiles = registry.list();
            if list_args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&profiles)
                        .expect("json serialization should work")
                );
            } else if profiles.is_empty() {
                println!("{}", palette.dim("no profiles found"));
            } else {
                println!("{}", palette.heading("Profiles"));
                let count = profiles.len();
                for (index, profile) in profiles.into_iter().enumerate() {
                    if index > 0 {
                        println!();
                    }
                    let profile_name = profile
                        .description
                        .as_deref()
                        .unwrap_or(profile.id.as_str());
                    let fields = vec![
                        ProfileField::new("name", profile_name),
                        ProfileField::new("id", profile.id.clone()),
                        ProfileField::new(
                            "planning",
                            format_profile_gate_mode(&profile.planning_mode),
                        ),
                        ProfileField::new(
                            "impl_review",
                            format_profile_gate_mode(&profile.implementation_review_mode),
                        ),
                        ProfileField::new("output", format_profile_output_mode(&profile.output)),
                        ProfileField::new("initial_state", profile.initial_state.clone()),
                        ProfileField::new("terminal_states", profile.terminal_states.join(", ")),
                    ];
                    for line in format_profile_fields(&fields, &palette) {
                        println!("{line}");
                    }
                }
                if count > 1 {
                    println!();
                }
                println!("{}", palette.dim(&format!("{count} profile(s)")));
            }
        }
        ProfileSubcommands::Show(show_args) => {
            let profile = registry.require(&show_args.id)?.clone();
            if show_args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&profile).expect("json serialization should work")
                );
            } else {
                println!("{}", palette.heading("Profile"));
                let mut fields = vec![
                    ProfileField::new("id", profile.id.clone()),
                    ProfileField::new("planning", format_profile_gate_mode(&profile.planning_mode)),
                    ProfileField::new(
                        "impl_review",
                        format_profile_gate_mode(&profile.implementation_review_mode),
                    ),
                    ProfileField::new("output", format_profile_output_mode(&profile.output)),
                    ProfileField::new("initial_state", profile.initial_state.clone()),
                    ProfileField::new("terminal_states", profile.terminal_states.join(", ")),
                ];
                if let Some(description) = profile.description.as_deref() {
                    fields.insert(1, ProfileField::new("description", description));
                }
                for line in format_profile_fields(&fields, &palette) {
                    println!("{line}");
                }
                println!("{}", palette.dim("workflow:"));
                for line in workflow_diagram::render(&profile) {
                    println!("  {line}");
                }
            }
        }
        ProfileSubcommands::SetDefault(set_default_args) => {
            let app = app::App::open(db_path, repo_root.to_path_buf())?;
            let profile_id = app.set_default_profile_id(&set_default_args.id)?;
            println!("default profile: {}", profile_id);
        }
        ProfileSubcommands::Set(set_args) => {
            let app = app::App::open(db_path, repo_root.to_path_buf())?;
            let profile = registry.require(&set_args.profile)?;
            let current = app
                .show_knot(&set_args.id)?
                .ok_or_else(|| app::AppError::NotFound(set_args.id.clone()))?;
            let state = resolve_profile_state_selection(
                profile,
                set_args.state.as_deref(),
                &current.state,
            )?;
            let knot = app.set_profile(
                &set_args.id,
                &profile.id,
                &state,
                set_args.if_match.as_deref(),
            )?;
            println!(
                "updated {} [{}] profile={}",
                knot_ref(&knot),
                knot.state,
                knot.profile_id
            );
        }
    }
    Ok(())
}

fn format_profile_output_mode(mode: &workflow::OutputMode) -> &'static str {
    match mode {
        workflow::OutputMode::Local => "Local",
        workflow::OutputMode::Remote => "Remote",
        workflow::OutputMode::Pr => "Pr",
        workflow::OutputMode::RemoteMain => "RemoteMain (merged)",
    }
}

fn format_profile_gate_mode(mode: &workflow::GateMode) -> &'static str {
    match mode {
        workflow::GateMode::Required => "Required",
        workflow::GateMode::Optional => "Optional",
        workflow::GateMode::Skipped => "Skipped",
    }
}

fn format_profile_fields(fields: &[ProfileField], palette: &ProfilePalette) -> Vec<String> {
    if fields.is_empty() {
        return Vec::new();
    }
    let label_width = fields
        .iter()
        .map(|field| field.label.len() + 1)
        .max()
        .unwrap_or(0);
    fields
        .iter()
        .map(|field| {
            let label = format!("{}:", field.label);
            let label_text = format!("{label:>label_width$}");
            format!("{}  {}", palette.label(&label_text), field.value)
        })
        .collect()
}

struct ProfileField {
    label: &'static str,
    value: String,
}

impl ProfileField {
    fn new(label: &'static str, value: impl Into<String>) -> Self {
        Self {
            label,
            value: value.into(),
        }
    }
}

struct ProfilePalette {
    enabled: bool,
}

impl ProfilePalette {
    fn auto() -> Self {
        let enabled = std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal();
        Self { enabled }
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.enabled {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    fn heading(&self, text: &str) -> String {
        self.paint("1;36", text)
    }

    fn label(&self, text: &str) -> String {
        self.paint("36", text)
    }

    fn dim(&self, text: &str) -> String {
        self.paint("2", text)
    }
}

fn resolve_profile_state_selection(
    profile: &workflow::ProfileDefinition,
    requested_state: Option<&str>,
    current_state: &str,
) -> Result<String, app::AppError> {
    let interactive = std::io::stdin().is_terminal();

    if let Some(raw_state) = requested_state {
        let state = normalize_cli_state(raw_state)?;
        if profile.require_state(&state).is_ok() {
            return Ok(state);
        }
        if !interactive {
            return Err(app::AppError::InvalidArgument(format!(
                "state '{}' is not valid for profile '{}'; valid states: {}",
                state,
                profile.id,
                profile.states.join(", ")
            )));
        }
        return prompt_for_profile_state(profile, current_state);
    }

    if !interactive {
        return Err(app::AppError::InvalidArgument(
            "--state is required in non-interactive mode".to_string(),
        ));
    }
    prompt_for_profile_state(profile, current_state)
}

fn prompt_for_profile_state(
    profile: &workflow::ProfileDefinition,
    current_state: &str,
) -> Result<String, app::AppError> {
    use std::io::{self, Write};

    if profile.states.is_empty() {
        return Err(app::AppError::InvalidArgument(format!(
            "profile '{}' has no valid states",
            profile.id
        )));
    }

    println!(
        "choose state for profile '{}' (knot currently '{}'):",
        profile.id, current_state
    );
    for (index, state) in profile.states.iter().enumerate() {
        println!("  {}. {}", index + 1, state);
    }

    let fallback_index = profile
        .states
        .iter()
        .position(|state| state == current_state)
        .or_else(|| {
            profile
                .states
                .iter()
                .position(|state| state == &profile.initial_state)
        })
        .unwrap_or(0);
    println!("press Enter to choose {}", profile.states[fallback_index]);

    let mut input = String::new();
    loop {
        print!("state [1-{}]: ", profile.states.len());
        io::stdout().flush()?;
        input.clear();
        io::stdin().read_line(&mut input)?;
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(profile.states[fallback_index].clone());
        }
        if let Ok(index) = trimmed.parse::<usize>() {
            if (1..=profile.states.len()).contains(&index) {
                return Ok(profile.states[index - 1].clone());
            }
        }
        println!("enter a number between 1 and {}", profile.states.len());
    }
}

fn normalize_cli_state(raw: &str) -> Result<String, app::AppError> {
    use std::str::FromStr;

    let parsed = domain::state::KnotState::from_str(raw)?;
    Ok(parsed.as_str().to_string())
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
        _ => Ok(None),
    }
}
