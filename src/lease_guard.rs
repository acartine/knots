use crate::app::{App, AppError, KnotView, StateActorMetadata};
use crate::domain::knot_type::KnotType;
use crate::lease_expiry::effective_lease_state;
use crate::workflow_runtime;

fn warn_invalid_lease_state(context: &str, detail: &str) {
    eprintln!("warning: {context}: {detail}");
}

fn load_bound_lease(app: &App, knot: &KnotView) -> Result<KnotView, AppError> {
    let Some(lease_id) = knot.lease_id.as_deref() else {
        return Err(AppError::InvalidArgument(
            "knot has no bound lease to release".to_string(),
        ));
    };

    let Some(lease_knot) = app.show_knot(lease_id)? else {
        warn_invalid_lease_state("invalid bound lease record", "lease knot is missing");
        return Err(AppError::InvalidArgument(
            "knot has a corrupt bound lease record".to_string(),
        ));
    };
    if lease_knot.knot_type != KnotType::Lease {
        warn_invalid_lease_state("invalid bound lease record", "bound knot is not a lease");
        return Err(AppError::InvalidArgument(
            "knot has a corrupt bound lease record".to_string(),
        ));
    }

    Ok(lease_knot)
}

pub(crate) fn validate_claim_external_lease(app: &App, lease_id: &str) -> Result<(), AppError> {
    let Some(lease_knot) = app.show_knot(lease_id)? else {
        warn_invalid_lease_state("claim rejected external lease", "lease knot is missing");
        return Err(AppError::InvalidArgument(
            "external lease was not found in local cache".to_string(),
        ));
    };
    if lease_knot.knot_type != KnotType::Lease {
        warn_invalid_lease_state("claim rejected external lease", "bound knot is not a lease");
        return Err(AppError::InvalidArgument(
            "external lease reference does not point to a lease knot".to_string(),
        ));
    }
    let state = effective_lease_state(&lease_knot.state, lease_knot.lease_expiry_ts);
    if state != workflow_runtime::LEASE_READY {
        warn_invalid_lease_state("claim rejected external lease", &lease_knot.state);
        return Err(AppError::InvalidArgument(format!(
            "external lease is in state '{}' -- expected '{}'",
            state,
            workflow_runtime::LEASE_READY
        )));
    }
    Ok(())
}

/// Validate that a bound lease is active before `kno next`.
///
/// **Exception**: if the lease has expired but is still bound to this
/// knot (nobody else claimed it), allow progression so the agent's
/// work is not wasted.
pub(crate) fn validate_next_bound_lease(
    app: &App,
    knot: &KnotView,
    provided_lease: Option<&str>,
) -> Result<(), AppError> {
    let Some(bound_lease) = knot.lease_id.as_deref() else {
        return match provided_lease {
            Some(lease_id) => Err(AppError::InvalidArgument(format!(
                "knot has no active lease but caller provided \
                 '{lease_id}'; lease binding is only allowed \
                 during claim operations"
            ))),
            None => Ok(()),
        };
    };

    let Some(provided_lease) = provided_lease else {
        return Err(AppError::InvalidArgument(
            "knot has a bound lease; rerun with --lease <lease-id>".to_string(),
        ));
    };
    if bound_lease != provided_lease {
        return Err(AppError::InvalidArgument(format!(
            "lease mismatch: knot has '{bound_lease}', \
             caller provided '{provided_lease}'"
        )));
    }

    let lease_knot = load_bound_lease(app, knot)?;
    let state = effective_lease_state(&lease_knot.state, lease_knot.lease_expiry_ts);
    if state == workflow_runtime::LEASE_ACTIVE {
        return Ok(());
    }

    // Exception: expired lease still bound → allow progression.
    // Only applies when raw state is NOT terminated (time-based expiry
    // caused the effective state change, not explicit termination).
    if state == workflow_runtime::LEASE_TERMINATED
        && lease_knot.state != workflow_runtime::LEASE_TERMINATED
        && knot.lease_id.as_deref() == Some(provided_lease)
    {
        return Ok(());
    }

    warn_invalid_lease_state("next rejected bound lease", &lease_knot.state);
    Err(AppError::InvalidArgument(format!(
        "bound lease is in state '{}' -- expected '{}'",
        state,
        workflow_runtime::LEASE_ACTIVE
    )))
}

/// Release (terminate + unbind) a lease bound to a knot.
///
/// Handles both active and expired-but-still-bound leases gracefully.
pub(crate) fn release_bound_lease(app: &App, knot_id: &str) -> Result<(), AppError> {
    let knot = app
        .show_knot(knot_id)?
        .ok_or_else(|| AppError::NotFound(knot_id.to_string()))?;
    let Some(lease_id) = knot.lease_id.as_deref() else {
        return Ok(());
    };

    let lease_knot = load_bound_lease(app, &knot)?;

    // If the lease is still active (or raw-active but expired),
    // terminate it. If already terminated in DB, skip termination.
    if lease_knot.state != workflow_runtime::LEASE_TERMINATED {
        crate::lease::terminate_lease(app, lease_id)?;
    }

    app.set_lease_id(knot_id, None)?;
    Ok(())
}

/// Materialize a single work knot's expired bound lease.
///
/// If the knot has a bound lease whose effective state is terminated but
/// whose raw DB state is still active/ready, this function terminates
/// the lease, unbinds it, and rolls the knot back to its prior queue
/// state. Returns `true` if materialization occurred.
pub(crate) fn materialize_expired_lease(app: &App, knot: &KnotView) -> Result<bool, AppError> {
    if knot.knot_type == KnotType::Lease {
        return Ok(false);
    }
    let Some(lease_id) = knot.lease_id.as_deref() else {
        return Ok(false);
    };
    let Some(lease_knot) = app.show_knot(lease_id)? else {
        return Ok(false);
    };
    if lease_knot.knot_type != KnotType::Lease {
        return Ok(false);
    }
    let effective = effective_lease_state(&lease_knot.state, lease_knot.lease_expiry_ts);
    if effective != workflow_runtime::LEASE_TERMINATED
        || lease_knot.state == workflow_runtime::LEASE_TERMINATED
    {
        return Ok(false);
    }

    // 1. Terminate the lease
    crate::lease::terminate_lease(app, lease_id)?;
    // 2. Unbind the lease from the work knot
    app.set_lease_id(&knot.id, None)?;
    // 3. Roll the work knot back to its prior queue state
    let resolution = crate::rollback::resolve_rollback_state(app, &knot.id)?;
    app.set_state_with_actor_and_options(
        &knot.id,
        &resolution.target_state,
        resolution.requires_force,
        None,
        StateActorMetadata::default(),
        false,
        false,
    )?;
    Ok(true)
}
