use crate::app::{App, AppError, KnotView};
use crate::domain::gate::GateData;
use crate::knot_id;
use crate::workflow::{self, OwnerKind};
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

pub fn resolve_next_state(
    app: &App,
    id: &str,
) -> Result<(KnotView, String, Option<&'static str>), AppError> {
    let knot = app
        .show_knot(id)?
        .ok_or_else(|| AppError::NotFound(id.into()))?;
    let registry = workflow::ProfileRegistry::load()?;
    let gate = knot.gate.clone().unwrap_or_else(GateData::default);
    let next = workflow_runtime::next_happy_path_state(
        &registry,
        &knot.profile_id,
        knot.knot_type,
        &knot.state,
    )?
    .ok_or_else(|| AppError::InvalidArgument(format!("no next state from '{}'", knot.state)))?;
    let owner = workflow_runtime::owner_kind_for_state(
        &registry,
        &knot.profile_id,
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
}
