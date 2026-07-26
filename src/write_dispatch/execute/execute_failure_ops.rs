//! `kno rollback --outcome <name>`: record a declared failure outcome.
//!
//! Generic rollback rewinds an action state to the queue state immediately
//! before it. That cannot express shipment's `merge_conflicts ->
//! ready_for_implementation`, which skips a whole phase. This path resolves
//! the target from the compiled workflow's failure map and applies it, the
//! lease unbind, and the failed step close as one atomic operation.

use crate::app::{App, AppError};
use crate::failure_outcome::{resolve_failure_outcome, FailureResolution};
use crate::lease_guard::validate_next_bound_lease;
use crate::write_queue::RollbackOperation;

use crate::write_dispatch::helpers::{format_json, format_rollback_output, lease_state_actor};

pub(super) fn execute_rollback_outcome(
    app: &App,
    args: &RollbackOperation,
    outcome: &str,
) -> Result<String, AppError> {
    let resolution = resolve_failure_outcome(app, &args.id, outcome)?;
    validate_next_bound_lease(app, &resolution.knot, args.lease_id.as_deref())?;

    if args.dry_run {
        return Ok(render(&resolution, &resolution.knot, args.json, true));
    }

    let actor = lease_state_actor(app, &resolution.knot.id, args.actor_kind.clone());
    let updated = app.fail_state_transition(
        &resolution.knot.id,
        &resolution.target_state,
        resolution.requires_force,
        actor,
    )?;
    Ok(render(&resolution, &updated, args.json, false))
}

fn render(
    resolution: &FailureResolution,
    knot: &crate::app::KnotView,
    json: bool,
    dry_run: bool,
) -> String {
    if json {
        return format_json(&serde_json::json!({
            "id": knot.id,
            "state": knot.state,
            "outcome": resolution.outcome,
            "target_state": resolution.target_state,
            "owner_kind": resolution.owner_kind,
            "reason": resolution.reason,
            "lease_released": !dry_run && resolution.knot.lease_id.is_some(),
            "dry_run": dry_run,
        }));
    }
    format_rollback_output(
        knot,
        &resolution.target_state,
        resolution.owner_kind,
        &resolution.reason,
        dry_run,
    )
}
