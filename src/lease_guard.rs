use crate::app::{App, AppError, KnotView};
use crate::domain::knot_type::KnotType;
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
    if lease_knot.state != workflow_runtime::LEASE_READY {
        warn_invalid_lease_state("claim rejected external lease", &lease_knot.state);
        return Err(AppError::InvalidArgument(format!(
            "external lease is in state '{}' -- expected '{}'",
            lease_knot.state,
            workflow_runtime::LEASE_READY
        )));
    }
    Ok(())
}

pub(crate) fn validate_next_bound_lease(
    app: &App,
    knot: &KnotView,
    provided_lease: Option<&str>,
) -> Result<(), AppError> {
    let Some(bound_lease) = knot.lease_id.as_deref() else {
        return match provided_lease {
            Some(lease_id) => Err(AppError::InvalidArgument(format!(
                "knot has no active lease but caller provided \
                 '{lease_id}'; lease binding is only allowed during claim operations"
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
            "lease mismatch: knot has '{bound_lease}', caller provided '{provided_lease}'"
        )));
    }

    let lease_knot = load_bound_lease(app, knot)?;
    if lease_knot.state != workflow_runtime::LEASE_ACTIVE {
        warn_invalid_lease_state("next rejected bound lease", &lease_knot.state);
        return Err(AppError::InvalidArgument(format!(
            "bound lease is in state '{}' -- expected '{}'",
            lease_knot.state,
            workflow_runtime::LEASE_ACTIVE
        )));
    }
    Ok(())
}

pub(crate) fn release_bound_lease(app: &App, knot_id: &str) -> Result<(), AppError> {
    let knot = app
        .show_knot(knot_id)?
        .ok_or_else(|| AppError::NotFound(knot_id.to_string()))?;
    let Some(lease_id) = knot.lease_id.as_deref() else {
        return Ok(());
    };

    let lease_knot = load_bound_lease(app, &knot)?;
    if lease_knot.state != workflow_runtime::LEASE_ACTIVE {
        warn_invalid_lease_state("failed to release bound lease", &lease_knot.state);
        return Err(AppError::InvalidArgument(format!(
            "bound lease is in state '{}' -- expected '{}'",
            lease_knot.state,
            workflow_runtime::LEASE_ACTIVE
        )));
    }

    crate::lease::terminate_lease(app, lease_id)?;
    app.set_lease_id(knot_id, None)?;
    let _ = app.trigger_queued_sync();
    Ok(())
}
