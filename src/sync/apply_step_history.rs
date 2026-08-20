use serde_json::Value;

use crate::app::helpers::apply_step_transition;
use crate::app::types::StateActorMetadata;

use super::apply_projection::MetadataProjection;

pub(super) fn apply_state_set_step_history(
    projection: &mut MetadataProjection,
    data: &serde_json::Map<String, Value>,
    occurred_at: &str,
) {
    let Some(to_state) = data.get("to").and_then(Value::as_str) else {
        return;
    };
    let actor = state_actor_from_event(data);
    let from_state = data
        .get("from")
        .and_then(Value::as_str)
        .unwrap_or(&projection.state);
    projection.step_history = apply_step_transition(
        &projection.step_history,
        from_state,
        to_state,
        occurred_at,
        &actor,
        projection.lease_id.as_deref(),
    );
    projection.state = to_state.to_string();
    projection.updated_at = occurred_at.to_string();
}

fn state_actor_from_event(data: &serde_json::Map<String, Value>) -> StateActorMetadata {
    StateActorMetadata {
        actor_kind: data
            .get("actor_kind")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        agent_name: data
            .get("agent_name")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        agent_model: data
            .get("agent_model")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        agent_version: data
            .get("agent_version")
            .and_then(Value::as_str)
            .map(ToString::to_string),
    }
}
