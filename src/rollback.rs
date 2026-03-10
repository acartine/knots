use crate::app::{App, AppError, KnotView};
use crate::dispatch::owner_kind_label;
use crate::profile::{DEFERRED, IMPLEMENTATION, PLANNING, SHIPMENT};
use crate::workflow::{self, ProfileDefinition};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollbackResolution {
    pub knot: KnotView,
    pub target_state: String,
    pub reason: String,
    pub requires_force: bool,
    pub owner_kind: Option<&'static str>,
}

pub fn resolve_rollback_state(app: &App, id: &str) -> Result<RollbackResolution, AppError> {
    let knot = app
        .show_knot(id)?
        .ok_or_else(|| AppError::NotFound(id.to_string()))?;
    let registry = workflow::ProfileRegistry::load()?;
    let profile = registry.require(&knot.profile_id)?;

    profile.require_state(&knot.state)?;
    reject_invalid_rollback_state(profile, &knot.state)?;

    let target = rollback_target(profile, &knot.state).ok_or_else(|| {
        AppError::InvalidArgument(format!("no rollback target from '{}'", knot.state))
    })?;
    let requires_force = profile
        .validate_transition(&knot.state, target.target_state, false)
        .is_err();
    let owner_kind = profile
        .owners
        .owner_kind_for_state(target.target_state)
        .map(owner_kind_label);

    Ok(RollbackResolution {
        knot,
        target_state: target.target_state.to_string(),
        reason: target.reason,
        requires_force,
        owner_kind,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RollbackTarget<'a> {
    target_state: &'a str,
    reason: String,
}

fn reject_invalid_rollback_state(profile: &ProfileDefinition, state: &str) -> Result<(), AppError> {
    if is_queue_state(state) {
        return Err(AppError::InvalidArgument(format!(
            "rollback is only allowed from action states; '{}' is a queue state",
            state
        )));
    }
    if profile.is_terminal_state(state) {
        return Err(AppError::InvalidArgument(format!(
            "rollback is only allowed from action states; '{}' is a terminal state",
            state
        )));
    }
    if state == DEFERRED {
        return Err(AppError::InvalidArgument(
            "rollback is only allowed from action states; 'deferred' is not actionable".to_string(),
        ));
    }
    Ok(())
}

fn rollback_target<'a>(
    profile: &'a ProfileDefinition,
    current: &str,
) -> Option<RollbackTarget<'a>> {
    let current_idx = profile.states.iter().position(|state| state == current)?;
    if is_review_action_state(current) {
        let action_idx = profile.states[..current_idx]
            .iter()
            .rposition(|state| is_non_review_action_state(state))?;
        let target_state = previous_ready_state(profile, action_idx)?;
        return Some(RollbackTarget {
            target_state,
            reason: format!(
                "{current} is a review state, so rollback skips the review loop and returns to \
                 {target_state}"
            ),
        });
    }
    if is_non_review_action_state(current) {
        let target_state = previous_ready_state(profile, current_idx)?;
        return Some(RollbackTarget {
            target_state,
            reason: format!(
                "{current} is an action state, so rollback returns to its preceding ready state \
                 {target_state}"
            ),
        });
    }
    None
}

fn previous_ready_state(profile: &ProfileDefinition, before_idx: usize) -> Option<&str> {
    profile.states[..before_idx]
        .iter()
        .rposition(|state| is_queue_state(state))
        .map(|idx| profile.states[idx].as_str())
}

fn is_queue_state(state: &str) -> bool {
    state.starts_with("ready_for_")
}

fn is_review_action_state(state: &str) -> bool {
    !is_queue_state(state) && state.ends_with("_review")
}

fn is_non_review_action_state(state: &str) -> bool {
    matches!(state, PLANNING | IMPLEMENTATION | SHIPMENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(id: &str) -> ProfileDefinition {
        workflow::ProfileRegistry::load()
            .expect("registry should load")
            .require(id)
            .expect("profile should exist")
            .clone()
    }

    fn profile_from_toml(raw: &str, id: &str) -> ProfileDefinition {
        workflow::ProfileRegistry::from_toml(raw)
            .expect("registry should load from toml")
            .require(id)
            .expect("profile should exist")
            .clone()
    }

    #[test]
    fn rollback_target_rewinds_non_review_action_states() {
        let profile = profile("autopilot");
        let target =
            rollback_target(&profile, "implementation").expect("implementation should roll back");
        assert_eq!(target.target_state, "ready_for_implementation");
        assert!(target.reason.contains("preceding ready state"));
    }

    #[test]
    fn rollback_target_rewinds_review_states_past_review_loop() {
        let profile = profile("autopilot");
        let implementation_review = rollback_target(&profile, "implementation_review")
            .expect("implementation review should roll back");
        assert_eq!(
            implementation_review.target_state,
            "ready_for_implementation"
        );
        assert!(implementation_review
            .reason
            .contains("skips the review loop"));

        let shipment_review =
            rollback_target(&profile, "shipment_review").expect("shipment review should roll back");
        assert_eq!(shipment_review.target_state, "ready_for_shipment");
    }

    #[test]
    fn rollback_target_honors_profiles_with_skipped_states() {
        let no_planning = profile("autopilot_no_planning");
        let implementation_review = rollback_target(&no_planning, "implementation_review")
            .expect("implementation review should roll back");
        assert_eq!(
            implementation_review.target_state,
            "ready_for_implementation"
        );

        let no_impl_review = profile_from_toml(
            r#"
                [[profiles]]
                id = "test_skipped_impl_review"
                planning_mode = "required"
                implementation_review_mode = "skipped"
                output = "local"

                [profiles.owners.planning]
                kind = "agent"
                [profiles.owners.plan_review]
                kind = "agent"
                [profiles.owners.implementation]
                kind = "agent"
                [profiles.owners.implementation_review]
                kind = "agent"
                [profiles.owners.shipment]
                kind = "agent"
                [profiles.owners.shipment_review]
                kind = "agent"
            "#,
            "test_skipped_impl_review",
        );
        let shipment =
            rollback_target(&no_impl_review, "shipment").expect("shipment should roll back");
        assert_eq!(shipment.target_state, "ready_for_shipment");
    }

    #[test]
    fn rollback_target_rejects_queue_terminal_and_deferred_states() {
        let profile = profile("autopilot");
        assert!(rollback_target(&profile, "ready_for_implementation").is_none());
        assert!(rollback_target(&profile, "shipped").is_none());
        assert!(rollback_target(&profile, "deferred").is_none());
    }
}
