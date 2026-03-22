use crate::app::{App, AppError, CreateKnotOptions, KnotView, StateActorMetadata};
use crate::domain::knot_type::KnotType;
use crate::domain::lease::{AgentInfo, LeaseData, LeaseType};

/// Create a lease knot in lease_ready state.
pub fn create_lease(
    app: &App,
    nickname: &str,
    lease_type: LeaseType,
    agent_info: Option<AgentInfo>,
) -> Result<KnotView, AppError> {
    let lease_data = LeaseData {
        lease_type,
        nickname: nickname.to_string(),
        agent_info,
    };
    let title = format!("Lease: {}", nickname);
    app.create_knot_with_options(
        &title,
        None,
        Some("lease_ready"),
        None,
        CreateKnotOptions {
            knot_type: KnotType::Lease,
            lease_data,
            ..CreateKnotOptions::default()
        },
    )
}

/// Transition a lease from lease_ready to lease_active.
pub fn activate_lease(app: &App, lease_id: &str) -> Result<KnotView, AppError> {
    app.set_state_with_actor(
        lease_id,
        "lease_active",
        false,
        None,
        StateActorMetadata::default(),
    )
}

/// Transition a lease to lease_terminated.
pub fn terminate_lease(app: &App, lease_id: &str) -> Result<KnotView, AppError> {
    app.set_state_with_actor(
        lease_id,
        "lease_terminated",
        false,
        None,
        StateActorMetadata::default(),
    )
}

/// List leases in lease_ready or lease_active state.
pub fn list_active_leases(app: &App) -> Result<Vec<KnotView>, AppError> {
    let all = app.list_knots()?;
    Ok(all
        .into_iter()
        .filter(|k| {
            k.knot_type == KnotType::Lease
                && matches!(k.state.as_str(), "lease_ready" | "lease_active")
        })
        .collect())
}

/// Bind a lease to a work/gate knot by setting its lease_id.
pub fn bind_lease(app: &App, knot_id: &str, lease_id: &str) -> Result<(), AppError> {
    app.set_lease_id(knot_id, Some(lease_id))
}

/// Unbind and terminate a lease from a knot.
pub fn unbind_lease(app: &App, knot_id: &str) -> Result<(), AppError> {
    let knot = app
        .show_knot(knot_id)?
        .ok_or_else(|| AppError::NotFound(knot_id.to_string()))?;
    if let Some(lid) = &knot.lease_id {
        let _ = terminate_lease(app, lid);
    }
    app.set_lease_id(knot_id, None)?;
    // Best-effort: run any queued sync now that a lease has ended.
    let _ = app.trigger_queued_sync();
    Ok(())
}
