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
    match knot_type {
        KnotType::Work => profile.initial_state.clone(),
        KnotType::Gate => READY_TO_EVALUATE.to_string(),
        KnotType::Lease => LEASE_READY.to_string(),
    }
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
    Ok(match knot_type {
        KnotType::Work => registry.require(profile_id)?.is_queue_state(state),
        KnotType::Gate => is_queue_state(state),
        KnotType::Lease => state == LEASE_READY,
    })
}

#[allow(dead_code)]
pub fn is_action_state_for_profile(
    registry: &ProfileRegistry,
    profile_id: &str,
    knot_type: KnotType,
    state: &str,
) -> Result<bool, ProfileError> {
    Ok(match knot_type {
        KnotType::Work => registry.require(profile_id)?.is_action_state(state),
        KnotType::Gate => is_action_state(state),
        KnotType::Lease => state == LEASE_ACTIVE,
    })
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
    match knot_type {
        KnotType::Work => Ok(registry.require(profile_id)?.is_terminal_state(state)),
        KnotType::Gate => Ok(matches!(state, "shipped" | "abandoned")),
        KnotType::Lease => Ok(matches!(state, LEASE_TERMINATED)),
    }
}

#[allow(dead_code)]
pub fn is_escape_state(
    registry: &ProfileRegistry,
    profile_id: &str,
    knot_type: KnotType,
    state: &str,
) -> Result<bool, ProfileError> {
    Ok(match knot_type {
        KnotType::Work => registry.require(profile_id)?.is_escape_state(state),
        KnotType::Gate | KnotType::Lease => false,
    })
}

pub fn validate_transition(
    registry: &ProfileRegistry,
    profile_id: &str,
    knot_type: KnotType,
    from: &str,
    to: &str,
    force: bool,
) -> Result<(), ProfileError> {
    match knot_type {
        KnotType::Work => registry
            .require(profile_id)?
            .validate_transition(from, to, force),
        KnotType::Gate => validate_gate_transition(from, to, force),
        KnotType::Lease => validate_lease_transition(from, to, force),
    }
}

pub fn next_happy_path_state(
    registry: &ProfileRegistry,
    profile_id: &str,
    knot_type: KnotType,
    current: &str,
) -> Result<Option<String>, ProfileError> {
    match knot_type {
        KnotType::Work => Ok(registry
            .require(profile_id)?
            .next_happy_path_state(current)
            .map(ToString::to_string)),
        KnotType::Gate => Ok(match current {
            READY_TO_EVALUATE => Some(EVALUATING.to_string()),
            EVALUATING => Some("shipped".to_string()),
            _ => None,
        }),
        KnotType::Lease => Ok(match current {
            LEASE_READY => Some(LEASE_ACTIVE.to_string()),
            LEASE_ACTIVE => Some(LEASE_TERMINATED.to_string()),
            _ => None,
        }),
    }
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
    if knot_type != KnotType::Work {
        return Ok(None);
    }

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
    match knot_type {
        KnotType::Work => Ok(registry
            .require(profile_id)?
            .owners
            .owner_kind_for_state(state)
            .cloned()),
        KnotType::Lease => Ok(None),
        KnotType::Gate => Ok(match state {
            READY_TO_EVALUATE | EVALUATING => Some(match gate.owner_kind {
                crate::domain::gate::GateOwnerKind::Human => OwnerKind::Human,
                crate::domain::gate::GateOwnerKind::Agent => OwnerKind::Agent,
            }),
            _ => None,
        }),
    }
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
    match knot_type {
        KnotType::Work => {
            let profile = registry.require(profile_id)?;
            let action = resolve_action_state(profile, state);
            Ok(action.map(|a| profile.step_metadata_for(a)))
        }
        KnotType::Gate => Ok(gate_step_metadata(state, gate)),
        KnotType::Lease => Ok(lease_step_metadata(state)),
    }
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

fn gate_step_metadata(state: &str, gate: &GateData) -> Option<StepMetadata> {
    match state {
        READY_TO_EVALUATE | EVALUATING => {
            let kind = match gate.owner_kind {
                crate::domain::gate::GateOwnerKind::Human => OwnerKind::Human,
                crate::domain::gate::GateOwnerKind::Agent => OwnerKind::Agent,
            };
            Some(StepMetadata {
                action_state: EVALUATING.to_string(),
                action_kind: Some("gate".to_string()),
                owner: Some(crate::workflow::StepOwner {
                    kind,
                    agent_name: None,
                    agent_model: None,
                    agent_version: None,
                }),
                output: None,
                review_hint: None,
            })
        }
        _ => None,
    }
}

fn lease_step_metadata(state: &str) -> Option<StepMetadata> {
    match state {
        LEASE_READY | LEASE_ACTIVE => Some(StepMetadata {
            action_state: LEASE_ACTIVE.to_string(),
            action_kind: Some("produce".to_string()),
            owner: None,
            output: None,
            review_hint: None,
        }),
        _ => None,
    }
}

/// Populate `step_metadata` and `next_step_metadata` on a KnotView
/// by resolving against the profile registry at read time.
pub fn enrich_step_metadata(knot: &mut crate::app::KnotView, registry: &ProfileRegistry) {
    let gate = knot.gate.clone().unwrap_or_default();
    let profile_id = crate::dispatch::profile_lookup_id(knot);
    knot.step_metadata =
        step_metadata_for_state(registry, &profile_id, knot.knot_type, &gate, &knot.state)
            .ok()
            .flatten();
    let next_state = next_happy_path_state(registry, &profile_id, knot.knot_type, &knot.state)
        .ok()
        .flatten();
    knot.next_step_metadata = next_state.and_then(|ns| {
        step_metadata_for_state(registry, &profile_id, knot.knot_type, &gate, &ns)
            .ok()
            .flatten()
    });
}

fn validate_gate_transition(from: &str, to: &str, force: bool) -> Result<(), ProfileError> {
    if force || from == to {
        return Ok(());
    }
    if matches!(to, "deferred" | "abandoned") {
        return Ok(());
    }
    let allowed = matches!(
        (from, to),
        (READY_TO_EVALUATE, EVALUATING) | (EVALUATING, "shipped")
    );
    if allowed {
        return Ok(());
    }
    Err(ProfileError::InvalidDefinition(format!(
        "invalid gate transition: {} -> {}",
        from, to
    )))
}

fn validate_lease_transition(from: &str, to: &str, force: bool) -> Result<(), ProfileError> {
    if force || from == to {
        return Ok(());
    }
    let allowed = matches!(
        (from, to),
        (LEASE_READY, LEASE_ACTIVE)
            | (LEASE_READY, LEASE_TERMINATED)
            | (LEASE_ACTIVE, LEASE_TERMINATED)
    );
    if allowed {
        return Ok(());
    }
    Err(ProfileError::InvalidDefinition(format!(
        "invalid lease transition: {} -> {}",
        from, to
    )))
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
