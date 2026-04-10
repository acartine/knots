use crate::domain::gate::GateData;
use crate::domain::knot_type::KnotType;
use crate::installed_workflows;
use crate::workflow::{OwnerKind, ProfileDefinition, ProfileError, ProfileRegistry, StepMetadata};

pub const READY_TO_EVALUATE: &str = "ready_to_evaluate";
pub const EVALUATING: &str = "evaluating";
pub const LEASE_READY: &str = "lease_ready";
pub const LEASE_ACTIVE: &str = "lease_active";
pub const LEASE_TERMINATED: &str = "lease_terminated";

pub fn initial_state(knot_type: KnotType, profile: &ProfileDefinition) -> String {
    let _ = knot_type;
    profile.initial_state.clone()
}

pub fn is_queue_state(state: &str) -> bool {
    state.starts_with("ready_for_") || state == READY_TO_EVALUATE || state == LEASE_READY
}

pub fn is_action_state(state: &str) -> bool {
    state == EVALUATING
        || (!is_queue_state(state)
            && !matches!(
                state,
                "shipped" | "abandoned" | "blocked" | "deferred" | LEASE_TERMINATED
            ))
}

pub fn is_queue_state_for_profile(
    registry: &ProfileRegistry,
    profile_id: &str,
    knot_type: KnotType,
    state: &str,
) -> Result<bool, ProfileError> {
    let _ = knot_type;
    Ok(registry.require(profile_id)?.is_queue_state(state))
}

#[allow(dead_code)]
pub fn is_action_state_for_profile(
    registry: &ProfileRegistry,
    profile_id: &str,
    knot_type: KnotType,
    state: &str,
) -> Result<bool, ProfileError> {
    let _ = knot_type;
    Ok(registry.require(profile_id)?.is_action_state(state))
}

#[allow(dead_code)]
pub fn queue_state_for_stage(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "planning" | "plan" => Some("ready_for_planning"),
        "plan_review" => Some("ready_for_plan_review"),
        "implementation" | "implement" => Some("ready_for_implementation"),
        "implementation_review" => Some("ready_for_implementation_review"),
        "shipment" | "ship" => Some("ready_for_shipment"),
        "shipment_review" => Some("ready_for_shipment_review"),
        "evaluate" | "evaluation" | "evaluating" | READY_TO_EVALUATE => Some(READY_TO_EVALUATE),
        _ => None,
    }
}

pub fn is_terminal_state(
    registry: &ProfileRegistry,
    profile_id: &str,
    knot_type: KnotType,
    state: &str,
) -> Result<bool, ProfileError> {
    let _ = knot_type;
    Ok(registry.require(profile_id)?.is_terminal_state(state))
}

#[allow(dead_code)]
pub fn is_escape_state(
    registry: &ProfileRegistry,
    profile_id: &str,
    knot_type: KnotType,
    state: &str,
) -> Result<bool, ProfileError> {
    let _ = knot_type;
    Ok(registry.require(profile_id)?.is_escape_state(state))
}

pub fn validate_transition(
    registry: &ProfileRegistry,
    profile_id: &str,
    knot_type: KnotType,
    from: &str,
    to: &str,
    force: bool,
) -> Result<(), ProfileError> {
    let _ = knot_type;
    registry
        .require(profile_id)?
        .validate_transition(from, to, force)
}

pub fn next_happy_path_state(
    registry: &ProfileRegistry,
    profile_id: &str,
    knot_type: KnotType,
    current: &str,
) -> Result<Option<String>, ProfileError> {
    let _ = knot_type;
    Ok(registry
        .require(profile_id)?
        .next_happy_path_state(current)
        .map(ToString::to_string))
}

pub fn next_outcome_state(
    registry: &ProfileRegistry,
    repo_root: &std::path::Path,
    workflow_id: &str,
    profile_id: &str,
    knot_type: KnotType,
    current: &str,
    outcome: &str,
) -> Result<Option<String>, ProfileError> {
    let normalized = outcome.trim().to_ascii_lowercase().replace('-', "_");
    if normalized.is_empty() || matches!(normalized.as_str(), "success" | "happy_path") {
        return next_happy_path_state(registry, profile_id, knot_type, current);
    }
    let _ = knot_type;

    let installed = installed_workflows::InstalledWorkflowRegistry::load(repo_root)?;
    let workflow = installed.require_workflow(workflow_id)?;
    Ok(workflow
        .prompt_for_action_state(current)
        .and_then(|prompt| {
            prompt
                .failure_targets
                .iter()
                .find(|(name, _)| name.trim().to_ascii_lowercase().replace('-', "_") == normalized)
                .map(|(_, target)| target.clone())
        }))
}

pub fn owner_kind_for_state(
    registry: &ProfileRegistry,
    profile_id: &str,
    knot_type: KnotType,
    gate: &GateData,
    state: &str,
) -> Result<Option<OwnerKind>, ProfileError> {
    let profile_owner = registry
        .require(profile_id)?
        .owners
        .owner_kind_for_state(state)
        .cloned();
    if knot_type == KnotType::Lease {
        return Ok(None);
    }
    if knot_type == KnotType::Gate && profile_owner.is_some() {
        return Ok(Some(match gate.owner_kind {
            crate::domain::gate::GateOwnerKind::Human => OwnerKind::Human,
            crate::domain::gate::GateOwnerKind::Agent => OwnerKind::Agent,
        }));
    }
    Ok(profile_owner)
}

/// Resolve step metadata for any workflow state. Queue states resolve
/// through their action state. Returns `None` for terminal/escape states.
pub fn step_metadata_for_state(
    registry: &ProfileRegistry,
    profile_id: &str,
    knot_type: KnotType,
    gate: &GateData,
    state: &str,
) -> Result<Option<StepMetadata>, ProfileError> {
    let profile = registry.require(profile_id)?;
    let action = resolve_action_state(profile, state);
    let mut metadata = action.map(|a| profile.step_metadata_for(a));
    if knot_type == KnotType::Gate {
        if let Some(step) = metadata.as_mut() {
            step.action_kind = Some("gate".to_string());
            if let Some(owner) = step.owner.as_mut() {
                owner.kind = match gate.owner_kind {
                    crate::domain::gate::GateOwnerKind::Human => OwnerKind::Human,
                    crate::domain::gate::GateOwnerKind::Agent => OwnerKind::Agent,
                };
            }
        }
    }
    Ok(metadata)
}

fn resolve_action_state<'a>(profile: &'a ProfileDefinition, state: &'a str) -> Option<&'a str> {
    if profile.is_queue_state(state) {
        profile.action_for_queue_state(state)
    } else if profile.is_terminal_state(state) || profile.is_escape_state(state) {
        None
    } else {
        Some(state)
    }
}

/// Populate `step_metadata` and `next_step_metadata` on a KnotView
/// by resolving against the profile registry at read time.
pub fn enrich_step_metadata(
    knot: &mut crate::app::KnotView,
    registry: &ProfileRegistry,
) -> Result<(), ProfileError> {
    let gate = knot.gate.clone().unwrap_or_default();
    let profile_id = crate::dispatch::profile_lookup_id(knot);
    if registry.require(&profile_id).is_err() {
        return Ok(());
    }
    knot.step_metadata =
        step_metadata_for_state(registry, &profile_id, knot.knot_type, &gate, &knot.state)?;
    let next_state = next_happy_path_state(registry, &profile_id, knot.knot_type, &knot.state)?;
    knot.next_step_metadata = next_state
        .map(|ns| step_metadata_for_state(registry, &profile_id, knot.knot_type, &gate, &ns))
        .transpose()?
        .flatten();
    Ok(())
}

#[cfg(test)]
#[path = "workflow_runtime_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "workflow_runtime_tests_ext.rs"]
mod tests_ext;

#[cfg(test)]
#[path = "step_metadata_tests.rs"]
mod step_metadata_tests;

#[cfg(test)]
#[path = "step_metadata_output_tests.rs"]
mod step_metadata_output_tests;
