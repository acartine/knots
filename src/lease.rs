use crate::app::{App, AppError, CreateKnotOptions, KnotView, StateActorMetadata};
use crate::domain::knot_type::KnotType;
use crate::domain::lease::{AgentInfo, LeaseData, LeaseType};

/// Create a lease knot in lease_ready state.
pub fn create_lease(
    app: &App,
    nickname: &str,
    lease_type: LeaseType,
    agent_info: Option<AgentInfo>,
    timeout_seconds: u64,
) -> Result<KnotView, AppError> {
    let lease_data = LeaseData {
        lease_type,
        nickname: nickname.to_string(),
        agent_info,
    };
    let title = format!("Lease: {}", nickname);
    let lease = app.create_knot_with_options(
        &title,
        None,
        Some("lease_ready"),
        None,
        None,
        CreateKnotOptions {
            knot_type: KnotType::Lease,
            lease_data,
            ..CreateKnotOptions::default()
        },
    )?;
    app.set_lease_expiry(
        &lease.id,
        crate::lease_expiry::compute_expiry_ts(timeout_seconds),
    )?;
    Ok(lease)
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

/// List leases whose effective state is lease_ready or lease_active.
pub fn list_active_leases(app: &App) -> Result<Vec<KnotView>, AppError> {
    let all = app.list_knots()?;
    Ok(all
        .into_iter()
        .filter(|k| {
            k.knot_type == KnotType::Lease
                && matches!(
                    crate::lease_expiry::effective_lease_state(&k.state, k.lease_expiry_ts,),
                    "lease_ready" | "lease_active"
                )
        })
        .collect())
}

/// Bind a lease to a work/gate knot by setting its lease_id.
pub fn bind_lease(app: &App, knot_id: &str, lease_id: &str) -> Result<(), AppError> {
    app.set_lease_id(knot_id, Some(lease_id))
}
