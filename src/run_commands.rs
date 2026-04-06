use crate::action_prompt;
use crate::cli::{
    ColdSubcommands, CompactArgs, DoctorArgs, FsckArgs, LeaseSubcommands, PerfArgs, SkillArgs,
};
use crate::{app, dispatch, domain, lease, list_layout, listing, stream_output};
use crate::{print_json, progress, progress_reporter, ui};

pub fn run_ls(app: &app::App, args: crate::cli::ListArgs) -> Result<(), app::AppError> {
    let filter = listing::KnotListFilter {
        include_all: args.all,
        state: args.state.clone(),
        knot_type: args.knot_type.clone(),
        profile_id: args.profile_id.clone(),
        tags: args.tags.clone(),
        query: args.query.clone(),
    };
    let mut knots = listing::apply_filters(app.list_knots()?, &filter);
    if let Some(limit) = args.limit {
        knots.truncate(limit);
    }
    if args.stream {
        stream_output::stream_ndjson_knots(&knots)
    } else if args.json {
        print_json(&knots);
        Ok(())
    } else {
        let layout_edges = crate::trace::measure("list_layout_edges", || app.list_layout_edges())?;
        let rows = crate::trace::measure("layout_knots", || {
            list_layout::layout_knots(knots, &layout_edges)
        });
        ui::print_knot_list(&rows, &filter);
        Ok(())
    }
}

pub fn run_show(app: &app::App, args: crate::cli::ShowArgs) -> Result<(), app::AppError> {
    match crate::trace::measure("show_knot", || app.show_knot(&args.id))? {
        Some(knot) => {
            reject_generic_lease_show(&knot, &args.id)?;
            if args.json {
                let mut value = show_json_value(&knot);
                trim_show_json_metadata(&mut value, &knot, args.verbose);
                print_json(&value);
            } else {
                ui::print_knot_show(&knot, args.verbose);
            }
            Ok(())
        }
        None => Err(app::AppError::NotFound(args.id)),
    }
}

fn reject_generic_lease_show(knot: &app::KnotView, id: &str) -> Result<(), app::AppError> {
    if knot.knot_type == domain::knot_type::KnotType::Lease {
        return Err(app::AppError::InvalidArgument(format!(
            "'{id}' is a lease knot; use `kno lease show {}` instead",
            knot.id
        )));
    }
    Ok(())
}

fn show_json_value(knot: &app::KnotView) -> serde_json::Value {
    let mut value = serde_json::to_value(knot).expect("json serialize");
    if let Some(object) = value.as_object_mut() {
        object.remove("lease_id");
    }
    value
}

fn trim_show_json_metadata(value: &mut serde_json::Value, knot: &app::KnotView, verbose: bool) {
    if !verbose {
        ui::trim_json_metadata(value, knot);
    }
}

pub fn run_pull(app: &app::App, args: crate::cli::SyncArgs) -> Result<(), app::AppError> {
    let mut reporter = progress_reporter(!args.json);
    let summary = app.pull_with_progress(
        reporter
            .as_mut()
            .map(|r| r as &mut dyn progress::ProgressReporter),
    )?;
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
    Ok(())
}

pub fn run_push(app: &app::App, args: crate::cli::SyncArgs) -> Result<(), app::AppError> {
    let mut reporter = progress_reporter(!args.json);
    let summary = app.push_with_progress(
        reporter
            .as_mut()
            .map(|r| r as &mut dyn progress::ProgressReporter),
    )?;
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
                .map(|c| format!(" commit={c}"))
                .unwrap_or_default()
        );
    }
    Ok(())
}

pub fn run_sync(app: &app::App, args: crate::cli::SyncArgs) -> Result<(), app::AppError> {
    use crate::replication::SyncOutcome;
    let mut reporter = progress_reporter(!args.json);
    let outcome = app.sync_or_defer_with_progress(
        reporter
            .as_mut()
            .map(|r| r as &mut dyn progress::ProgressReporter),
    )?;
    match outcome {
        SyncOutcome::Completed(summary) => {
            if args.json {
                print_json(&summary);
            } else {
                println!(
                    "sync push(local_event_files={} copied_files={} \
                     committed={} pushed={}) \
                     pull(head={} index_files={} full_files={} \
                     knot_updates={} edge_adds={} edge_removes={})",
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
        SyncOutcome::Deferred { active_leases } => {
            if args.json {
                print_json(&outcome);
            } else {
                println!(
                    "sync deferred: {} active lease(s); \
                     sync will run when leases are terminated",
                    active_leases
                );
            }
        }
    }
    Ok(())
}

pub fn run_fsck(app: &app::App, args: FsckArgs) -> Result<(), app::AppError> {
    let report = crate::trace::measure("fsck", || app.fsck())?;
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
    Ok(())
}

pub fn run_doctor(app: &app::App, args: DoctorArgs) -> Result<(), app::AppError> {
    let report = app.doctor(args.fix)?;
    if args.json {
        print_json(&report);
    } else {
        ui::print_doctor_report(&report);
    }
    if !args.fix && crate::doctor_fix::has_non_pass_checks(&report.checks) {
        eprintln!("kno doctor --fix to address these items");
    }
    if report.failure_count() > 0 {
        return Err(app::AppError::InvalidArgument(format!(
            "doctor found {} failing check(s)",
            report.failure_count()
        )));
    }
    Ok(())
}

pub fn run_perf(app: &app::App, args: PerfArgs) -> Result<(), app::AppError> {
    let report = app.perf_harness(args.iterations)?;
    if args.json {
        print_json(&report);
    } else {
        println!("perf iterations={}", report.iterations);
        for m in &report.measurements {
            println!(
                "  {} elapsed_ms={:.2} budget_ms={:.2} within_budget={}",
                m.name, m.elapsed_ms, m.budget_ms, m.within_budget
            );
        }
    }
    if args.strict && report.over_budget_count() > 0 {
        return Err(app::AppError::InvalidArgument(format!(
            "perf regression: {} measurement(s) over budget",
            report.over_budget_count()
        )));
    }
    Ok(())
}

pub fn run_compact(app: &app::App, args: CompactArgs) -> Result<(), app::AppError> {
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
    Ok(())
}

pub fn run_cold(app: &app::App, args: crate::cli::ColdArgs) -> Result<(), app::AppError> {
    match args.command {
        ColdSubcommands::Sync(sync_args) => {
            let summary = crate::trace::measure("cold_sync", || app.cold_sync())?;
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
            let matches =
                crate::trace::measure("cold_search", || app.cold_search(&search_args.term))?;
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
    }
    Ok(())
}

pub fn run_rehydrate(app: &app::App, args: crate::cli::RehydrateArgs) -> Result<(), app::AppError> {
    match crate::trace::measure("rehydrate", || app.rehydrate(&args.id))? {
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
            Ok(())
        }
        None => Err(app::AppError::NotFound(args.id)),
    }
}

pub fn run_edge_list(
    app: &app::App,
    edge_args: crate::cli::EdgeListArgs,
) -> Result<(), app::AppError> {
    let edges = crate::trace::measure("list_edges", || {
        app.list_edges(&edge_args.id, &edge_args.direction)
    })?;
    if edge_args.json {
        print_json(&edges);
    } else if edges.is_empty() {
        println!("no edges for {}", edge_args.id);
    } else {
        for edge in edges {
            println!("{} -[{}]-> {}", edge.src, edge.kind, edge.dst);
        }
    }
    Ok(())
}

pub fn run_skill(app: &app::App, args: SkillArgs) -> Result<(), app::AppError> {
    let content = crate::trace::measure("resolve_skill", || match app.show_knot(&args.id)? {
        Some(knot) => resolve_skill_for_knot(app, &knot, &args.id),
        None => resolve_skill_by_name(app, &args.id),
    })?;
    print!("{content}");
    Ok(())
}

fn resolve_skill_for_knot(
    app: &app::App,
    knot: &app::KnotView,
    id: &str,
) -> Result<String, app::AppError> {
    let (_knot, next, _owner) = dispatch::resolve_next_state(app, id)?;
    let profile = app
        .profile_registry()
        .require(&dispatch::profile_lookup_id(knot))?;
    action_prompt::render_for_profile(profile, &next).ok_or_else(|| {
        app::AppError::InvalidArgument(format!("'{}' is not a knot id or skill state name", id))
    })
}

fn resolve_skill_by_name(app: &app::App, id: &str) -> Result<String, app::AppError> {
    let normalized = id.trim().to_ascii_lowercase().replace('-', "_");
    let workflow_id = app.default_workflow_id()?;
    let profile_id = app.default_profile_id_for_workflow(&workflow_id)?;
    let profile = app.profile_registry().require(&profile_id)?;
    action_prompt::render_for_profile(profile, &normalized).ok_or_else(|| {
        app::AppError::InvalidArgument(format!("'{}' is not a knot id or skill state name", id))
    })
}

pub fn run_lease_read(app: &app::App, args: crate::cli::LeaseArgs) -> Result<(), app::AppError> {
    match args.command {
        LeaseSubcommands::Show(ref show) => {
            let knot = crate::trace::measure("lease_show", || app.show_knot(&show.id))?
                .ok_or_else(|| app::AppError::NotFound(show.id.clone()))?;
            if show.json {
                print_json(&knot);
            } else {
                ui::print_knot_show(&knot, false);
            }
        }
        LeaseSubcommands::List(ref list) => {
            let leases = crate::trace::measure("lease_list", || {
                if list.all {
                    Ok(app
                        .list_knots()?
                        .into_iter()
                        .filter(|k| k.knot_type == domain::knot_type::KnotType::Lease)
                        .collect::<Vec<_>>())
                } else {
                    lease::list_active_leases(app)
                }
            })?;
            if list.json {
                print_json(&leases);
            } else if leases.is_empty() {
                println!("no active leases");
            } else {
                let palette = ui::Palette::auto();
                for l in &leases {
                    println!(
                        "{} {} {}",
                        palette.id(&l.id),
                        palette.state(&l.state),
                        l.title
                    );
                }
            }
        }
        _ => unreachable!("lease write commands handled before app init"),
    }
    Ok(())
}

#[cfg(test)]
#[path = "run_commands_tests.rs"]
mod tests;
