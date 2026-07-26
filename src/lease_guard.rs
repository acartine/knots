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

pub(crate) fn validate_bindable_external_lease(app: &App, lease_id: &str) -> Result<(), AppError> {
    let Some(lease_knot) = app.show_knot(lease_id)? else {
        warn_invalid_lease_state("lease binding rejected", "lease knot is missing");
        return Err(AppError::InvalidArgument(
            "external lease was not found in local cache".to_string(),
        ));
    };
    if lease_knot.knot_type != KnotType::Lease {
        warn_invalid_lease_state("lease binding rejected", "bound knot is not a lease");
        return Err(AppError::InvalidArgument(
            "external lease reference does not point to a lease knot".to_string(),
        ));
    }
    let state = effective_lease_state(&lease_knot.state, lease_knot.lease_expiry_ts);
    if matches!(
        state,
        workflow_runtime::LEASE_READY | workflow_runtime::LEASE_ACTIVE
    ) {
        return Ok(());
    }
    warn_invalid_lease_state("lease binding rejected", &lease_knot.state);
    Err(AppError::InvalidArgument(format!(
        "external lease is in state '{}' -- expected '{}' or '{}'",
        state,
        workflow_runtime::LEASE_READY,
        workflow_runtime::LEASE_ACTIVE
    )))
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

    // MCP owns a session-scoped lease. Release it back to ready so the next
    // action in the same MCP session can carry the same attributed identity.
    if lease_knot.state != workflow_runtime::LEASE_TERMINATED {
        if crate::lease::is_mcp_session_lease(&lease_knot) {
            ready_mcp_session_lease(app, &lease_knot)?;
        } else {
            crate::lease::terminate_lease(app, lease_id)?;
        }
    }

    app.set_lease_id(knot_id, None)?;
    Ok(())
}

fn ready_mcp_session_lease(app: &App, lease: &KnotView) -> Result<(), AppError> {
    app.set_state_with_actor(
        &lease.id,
        workflow_runtime::LEASE_READY,
        true,
        None,
        StateActorMetadata::default(),
    )?;
    let timeout = lease
        .lease
        .as_ref()
        .and_then(|data| data.timeout_seconds)
        .unwrap_or(crate::lease_expiry::DEFAULT_LEASE_TIMEOUT_SECONDS);
    app.set_lease_expiry(&lease.id, crate::lease_expiry::compute_expiry_ts(timeout))?;
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
    app.recover_expired_bound_lease(&knot.id)
}
