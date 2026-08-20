use crate::app::{App, AppError, CreateKnotOptions, KnotView, StateActorMetadata};
use crate::domain::knot_type::KnotType;
use crate::domain::lease::{AgentInfo, LeaseData, LeaseOwner, LeaseType};

const MCP_SESSION_LEASE_PREFIX: &str = "mcp-";

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
        timeout_seconds: Some(timeout_seconds),
        owner: Some(LeaseOwner {
            machine_id: app.machine_id(),
            pid: std::process::id(),
        }),
    };
    let title = lease_title(nickname);
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

pub(crate) fn is_mcp_session_lease(knot: &KnotView) -> bool {
    knot.lease
        .as_ref()
        .is_some_and(|data| data.nickname.starts_with(MCP_SESSION_LEASE_PREFIX))
}

fn lease_title(nickname: &str) -> String {
    if nickname.starts_with(MCP_SESSION_LEASE_PREFIX) {
        nickname.to_string()
    } else {
        format!("Lease: {}", nickname)
    }
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
    app.terminate_lease_and_recover_bound_action(lease_id)
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

/// List active leases held by this machine.
///
/// Leases replicated from another machine are excluded: no agent here holds
/// them. A lease with no recorded owner predates owner tracking and is
/// treated as local.
pub fn list_local_active_leases(app: &App) -> Result<Vec<KnotView>, AppError> {
    let machine_id = app.machine_id();
    Ok(list_active_leases(app)?
        .into_iter()
        .filter(|k| {
            k.lease
                .as_ref()
                .is_none_or(|data| data.is_owned_by(&machine_id))
        })
        .collect())
}

/// Bind a lease to a work/gate knot by setting its lease_id.
pub fn bind_lease(app: &App, knot_id: &str, lease_id: &str) -> Result<(), AppError> {
    app.set_lease_id(knot_id, Some(lease_id))
}
