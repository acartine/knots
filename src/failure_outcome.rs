//! Resolve a named workflow failure outcome to its declared target state.
//!
//! Each action prompt declares where its failure outcomes land (shipment's
//! `merge_conflicts` -> `ready_for_implementation`, for example). Generic
//! rollback only knows "action state -> immediately preceding queue state",
//! which cannot express a non-adjacent failure target. This module reads the
//! compiled workflow's failure map so `kno rollback --outcome` can move a
//! knot exactly where the workflow says a failure belongs — and nowhere
//! else. The target is never caller-supplied, so this stays a constrained
//! transition rather than a force-state escape hatch.

use crate::app::{App, AppError, KnotView};
use crate::dispatch::{owner_kind_label, profile_lookup_id};
use crate::installed_workflows::InstalledWorkflowRegistry;
use crate::rollback::{reject_invalid_rollback_state, require_rollback_state};
use crate::workflow_runtime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureResolution {
    pub knot: KnotView,
    pub outcome: String,
    pub target_state: String,
    pub reason: String,
    pub requires_force: bool,
    pub owner_kind: Option<&'static str>,
}

/// Normalize an outcome name the way the compiled bundle keys them.
fn normalize(outcome: &str) -> String {
    outcome.trim().to_ascii_lowercase().replace('-', "_")
}

pub fn resolve_failure_outcome(
    app: &App,
    id: &str,
    outcome: &str,
) -> Result<FailureResolution, AppError> {
    let knot = app
        .show_knot(id)?
        .ok_or_else(|| AppError::NotFound(id.to_string()))?;
    let registry = app.profile_registry();
    let profile_id = profile_lookup_id(&knot);
    let profile = registry.require(&profile_id)?;
    let gate = knot.gate.clone().unwrap_or_default();

    // Failure outcomes are only meaningful from an action state, and the
    // rejection wording is shared with generic rollback on purpose.
    require_rollback_state(profile, knot.knot_type, &knot.state)?;
    reject_invalid_rollback_state(profile, knot.knot_type, &knot.state)?;

    let requested = normalize(outcome);
    if requested.is_empty() {
        return Err(AppError::InvalidArgument(
            "--outcome requires a failure outcome name".to_string(),
        ));
    }
    let target_state = failure_target(app, &knot, &requested)?;

    let requires_force = workflow_runtime::validate_transition(
        registry,
        &profile_id,
        knot.knot_type,
        &knot.state,
        &target_state,
        false,
    )
    .is_err();
    let owner_kind = workflow_runtime::owner_kind_for_state(
        registry,
        &profile_id,
        knot.knot_type,
        &gate,
        &target_state,
    )?
    .map(|kind| owner_kind_label(&kind));
    let reason = format!(
        "{} declares failure outcome '{}' -> {}",
        knot.state, requested, target_state
    );

    Ok(FailureResolution {
        knot,
        outcome: requested,
        target_state,
        reason,
        requires_force,
        owner_kind,
    })
}

fn failure_target(app: &App, knot: &KnotView, requested: &str) -> Result<String, AppError> {
    let installed = InstalledWorkflowRegistry::load(app.repo_root())?;
    let workflow = installed.require_workflow(&knot.workflow_id)?;
    let prompt = workflow
        .prompt_for_action_state(&knot.state)
        .ok_or_else(|| {
            AppError::InvalidArgument(format!(
                "workflow '{}' declares no prompt for action state '{}'",
                knot.workflow_id, knot.state
            ))
        })?;

    if let Some(target) = prompt
        .failure_targets
        .iter()
        .find(|(name, _)| normalize(name) == requested)
        .map(|(_, target)| target.clone())
    {
        return Ok(target);
    }

    // A success request is a legitimate name but not a failure; send the
    // caller to `kno next` instead of silently advancing them here.
    if matches!(requested, "success" | "happy_path") {
        return Err(AppError::InvalidArgument(format!(
            "'{requested}' is not a failure outcome of '{}'; use `kno next` to advance",
            knot.state
        )));
    }

    Err(AppError::InvalidArgument(format!(
        "'{requested}' is not a declared failure outcome of '{}'{}",
        knot.state,
        available_outcomes(prompt)
    )))
}

fn available_outcomes(prompt: &crate::installed_workflows::PromptDefinition) -> String {
    if prompt.failure_targets.is_empty() {
        return "; it declares no failure outcomes".to_string();
    }
    let names: Vec<&str> = prompt
        .failure_targets
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    format!("; declared failure outcomes: {}", names.join(", "))
}
