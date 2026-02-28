use crate::app::{App, AppError, KnotView};
use crate::knot_id;
use crate::workflow;

pub fn knot_ref(knot: &KnotView) -> String {
    let sid = knot_id::display_id(&knot.id);
    knot.alias
        .as_deref()
        .map_or(sid.to_string(), |a| format!("{a} ({sid})"))
}

pub fn resolve_next_state(app: &App, id: &str) -> Result<(KnotView, String), AppError> {
    let knot = app
        .show_knot(id)?
        .ok_or_else(|| AppError::NotFound(id.into()))?;
    let registry = workflow::ProfileRegistry::load()?;
    let profile = registry.require(&knot.profile_id)?;
    let next = profile
        .next_happy_path_state(&knot.state)
        .ok_or_else(|| AppError::InvalidArgument(format!("no next state from '{}'", knot.state)))?;
    Ok((knot, next.to_string()))
}
