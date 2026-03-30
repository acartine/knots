use crate::app::{App, AppError, KnotView};
use crate::cli::ReadyArgs;
use crate::dispatch::profile_lookup_id;
use crate::domain::knot_type::KnotType;
use crate::listing::{apply_filters, KnotListFilter};
use crate::workflow::{OwnerKind, ProfileRegistry};
use crate::workflow_runtime;

pub fn run_ready(app: &App, args: ReadyArgs) -> Result<(), AppError> {
    let stage = normalize_ready_type(args.ready_type.as_deref());
    let candidates = list_queue_candidates(app, stage.as_deref())?;
    if args.json {
        let json =
            serde_json::to_string_pretty(&candidates).expect("JSON serialization should work");
        println!("{json}");
    } else if candidates.is_empty() {
        println!("no knots ready for action");
    } else {
        let palette = crate::ui::Palette::auto();
        for knot in &candidates {
            let sid = crate::knot_id::display_id(&knot.id);
            let display_id = knot
                .alias
                .as_deref()
                .map_or(sid.to_string(), |a| format!("{a} ({sid})"));
            println!(
                "{} {} {}",
                palette.id(&display_id),
                palette.state(&knot.state),
                knot.title
            );
        }
    }
    Ok(())
}

pub fn list_queue_candidates(
    app: &App,
    stage: Option<&str>,
) -> Result<Vec<KnotView>, AppError> {
    let filter = KnotListFilter {
        include_all: false,
        state: None,
        knot_type: None,
        profile_id: None,
        tags: Vec::new(),
        query: None,
    };
    let mut knots = apply_filters(app.list_knots()?, &filter);
    let registry = app.profile_registry();
    knots.retain(|k| {
        matches!(
            workflow_runtime::is_queue_state_for_profile(
                registry,
                &k.profile_id,
                k.knot_type,
                &k.state
            ),
            Ok(true)
        )
    });
    if let Some(stage) = stage {
        let normalized =
            normalize_ready_type(Some(stage)).unwrap_or_else(|| stage.to_string());
        knots.retain(|k| queue_stage_matches(registry, k, &normalized));
    }
    knots.retain(|k| k.knot_type != KnotType::Lease);
    knots.sort_by(|a, b| {
        let pa = a.priority.unwrap_or(i64::MAX);
        let pb = b.priority.unwrap_or(i64::MAX);
        pa.cmp(&pb).then_with(|| a.updated_at.cmp(&b.updated_at))
    });
    Ok(knots)
}

pub fn queue_stage_matches(
    registry: &ProfileRegistry,
    knot: &KnotView,
    normalized: &str,
) -> bool {
    if knot.state == normalized {
        return true;
    }
    if knot.state.trim_start_matches("ready_for_") == normalized {
        return true;
    }
    if normalized == "evaluate" && knot.state == workflow_runtime::READY_TO_EVALUATE {
        return true;
    }
    if let Ok(profile) = registry.require(&profile_lookup_id(knot)) {
        return profile.queue_states.iter().any(|state| {
            state == &knot.state
                && (state.trim_start_matches("ready_for_") == normalized
                    || profile.action_for_queue_state(state) == Some(normalized))
        });
    }
    false
}

pub fn normalize_ready_type(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lowered = trimmed.to_ascii_lowercase().replace('-', "_");
    if lowered == workflow_runtime::READY_TO_EVALUATE {
        Some("evaluate".to_string())
    } else if lowered.starts_with("ready_for_") {
        Some(lowered.trim_start_matches("ready_for_").to_string())
    } else {
        Some(lowered)
    }
}

pub fn parse_owner_filter(raw: Option<&str>) -> OwnerKind {
    match raw.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("human") => OwnerKind::Human,
        _ => OwnerKind::Agent,
    }
}
