use crate::app::{App, AppError, KnotView};
use crate::domain::gate::GateData;
use crate::knot_id;
use crate::workflow::OwnerKind;
use crate::workflow_runtime;

pub fn knot_ref(knot: &KnotView) -> String {
    let sid = knot_id::display_id(&knot.id);
    knot.alias
        .as_deref()
        .map_or(sid.to_string(), |a| format!("{a} ({sid})"))
}

pub fn owner_kind_label(kind: &OwnerKind) -> &'static str {
    match kind {
        OwnerKind::Human => "human",
        OwnerKind::Agent => "agent",
    }
}

pub fn profile_lookup_id(knot: &KnotView) -> String {
    if !crate::installed_workflows::is_builtin_workflow_id(&knot.workflow_id)
        && !knot.profile_id.contains('/')
    {
        format!("{}/{}", knot.workflow_id, knot.profile_id)
    } else {
        knot.profile_id.clone()
    }
}

pub fn resolve_next_state(
    app: &App,
    id: &str,
) -> Result<(KnotView, String, Option<&'static str>), AppError> {
    let knot = app
        .show_knot(id)?
        .ok_or_else(|| AppError::NotFound(id.into()))?;
    let registry = app.profile_registry();
    let gate = knot.gate.clone().unwrap_or_else(GateData::default);
    let profile_id = profile_lookup_id(&knot);
    let next = workflow_runtime::next_happy_path_state(
        registry,
        &profile_id,
        knot.knot_type,
        &knot.state,
    )?
    .ok_or_else(|| AppError::InvalidArgument(format!("no next state from '{}'", knot.state)))?;
    let owner = workflow_runtime::owner_kind_for_state(
        registry,
        &profile_id,
        knot.knot_type,
        &gate,
        &next,
    )?
    .as_ref()
    .map(owner_kind_label);
    Ok((knot, next, owner))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_kind_label_covers_human_and_agent() {
        assert_eq!(owner_kind_label(&OwnerKind::Human), "human");
        assert_eq!(owner_kind_label(&OwnerKind::Agent), "agent");
    }

    #[test]
    fn profile_lookup_id_prefixes_non_builtin_workflow() {
        let knot = KnotView {
            id: "knots-1".to_string(),
            alias: None,
            title: "Test".to_string(),
            state: "planning".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            body: None,
            description: None,
            acceptance: None,
            priority: None,
            knot_type: crate::domain::knot_type::KnotType::Work,
            tags: Vec::new(),
            notes: Vec::new(),
            handoff_capsules: Vec::new(),
            invariants: Vec::new(),
            step_history: Vec::new(),
            gate: None,
            lease: None,
            lease_id: None,
            lease_expiry_ts: 0,
            lease_agent: None,
            workflow_id: "custom-wf".to_string(),
            profile_id: "autopilot".to_string(),
            profile_etag: None,
            deferred_from_state: None,
            blocked_from_state: None,
            created_at: None,
            step_metadata: None,
            next_step_metadata: None,
            edges: Vec::new(),
            child_summaries: Vec::new(),
        };
        assert_eq!(profile_lookup_id(&knot), "custom-wf/autopilot");
    }

    #[test]
    fn profile_lookup_id_returns_as_is_for_builtin() {
        let knot = KnotView {
            id: "knots-2".to_string(),
            alias: None,
            title: "Compat".to_string(),
            state: "planning".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            body: None,
            description: None,
            acceptance: None,
            priority: None,
            knot_type: crate::domain::knot_type::KnotType::Work,
            tags: Vec::new(),
            notes: Vec::new(),
            handoff_capsules: Vec::new(),
            invariants: Vec::new(),
            step_history: Vec::new(),
            gate: None,
            lease: None,
            lease_id: None,
            lease_expiry_ts: 0,
            lease_agent: None,
            workflow_id: "knots_sdlc".to_string(),
            profile_id: "default".to_string(),
            profile_etag: None,
            deferred_from_state: None,
            blocked_from_state: None,
            created_at: None,
            step_metadata: None,
            next_step_metadata: None,
            edges: Vec::new(),
            child_summaries: Vec::new(),
        };
        assert_eq!(profile_lookup_id(&knot), "default");
    }
}
