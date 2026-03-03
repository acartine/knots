use crate::app::{App, AppError, KnotView};
use crate::knot_id;
use crate::workflow::{self, OwnerKind};

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
    let profile = registry.require(&knot.profile_id)?;
    let next = profile
        .next_happy_path_state(&knot.state)
        .ok_or_else(|| AppError::InvalidArgument(format!("no next state from '{}'", knot.state)))?;
    let owner = profile
        .owners
        .owner_kind_for_state(next)
        .map(owner_kind_label);
    Ok((knot, next.to_string(), owner))
}
