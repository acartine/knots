use crate::domain::gate::GateData;
use crate::domain::knot_type::KnotType;
use crate::installed_workflows;
use crate::workflow::{OwnerKind, ProfileDefinition, ProfileError, ProfileRegistry};

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
                "shipped" | "abandoned" | "deferred" | LEASE_TERMINATED
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
mod tests {
    use super::{
        initial_state, is_action_state, is_action_state_for_profile, is_queue_state,
        is_queue_state_for_profile, is_terminal_state, next_happy_path_state, next_outcome_state,
        owner_kind_for_state, queue_state_for_stage, validate_transition, EVALUATING,
        READY_TO_EVALUATE,
    };
    use crate::domain::gate::{GateData, GateOwnerKind};
    use crate::domain::knot_type::KnotType;
    use crate::installed_workflows;
    use crate::workflow::{OwnerKind, ProfileRegistry};
    use uuid::Uuid;

    fn unique_workspace(prefix: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&path).expect("workspace should be creatable");
        path
    }

    #[test]
    fn gate_states_have_explicit_queue_and_action_classification() {
        assert!(is_queue_state(READY_TO_EVALUATE));
        assert!(is_action_state(EVALUATING));
        assert!(!is_action_state(READY_TO_EVALUATE));
    }

    #[test]
    fn queue_state_for_stage_maps_gate_aliases() {
        assert_eq!(queue_state_for_stage("evaluate"), Some(READY_TO_EVALUATE));
        assert_eq!(queue_state_for_stage("evaluating"), Some(READY_TO_EVALUATE));
    }

    #[test]
    fn gate_next_happy_path_is_fixed() {
        let registry = ProfileRegistry::load().unwrap();
        assert_eq!(
            next_happy_path_state(&registry, "autopilot", KnotType::Gate, READY_TO_EVALUATE)
                .unwrap(),
            Some(EVALUATING.to_string())
        );
        assert_eq!(
            next_happy_path_state(&registry, "autopilot", KnotType::Gate, EVALUATING).unwrap(),
            Some("shipped".to_string())
        );
    }

    #[test]
    fn gate_owner_kind_comes_from_gate_data() {
        let registry = ProfileRegistry::load().unwrap();
        let gate = GateData {
            owner_kind: GateOwnerKind::Human,
            ..Default::default()
        };
        assert_eq!(
            owner_kind_for_state(
                &registry,
                "autopilot",
                KnotType::Gate,
                &gate,
                READY_TO_EVALUATE
            )
            .unwrap(),
            Some(OwnerKind::Human)
        );
    }

    #[test]
    fn initial_state_uses_gate_queue_for_gate_knots() {
        let registry = ProfileRegistry::load().unwrap();
        let profile = registry.require("autopilot").unwrap();
        assert_eq!(initial_state(KnotType::Gate, profile), READY_TO_EVALUATE);
    }

    #[test]
    fn gate_terminal_state_and_transition_rules_are_fixed() {
        let registry = ProfileRegistry::load().unwrap();
        assert!(
            !is_terminal_state(&registry, "autopilot", KnotType::Gate, READY_TO_EVALUATE).unwrap()
        );
        assert!(is_terminal_state(&registry, "autopilot", KnotType::Gate, "shipped").unwrap());
        assert!(validate_transition(
            &registry,
            "autopilot",
            KnotType::Gate,
            READY_TO_EVALUATE,
            EVALUATING,
            false
        )
        .is_ok());
        assert!(validate_transition(
            &registry,
            "autopilot",
            KnotType::Gate,
            EVALUATING,
            "shipped",
            false
        )
        .is_ok());
        let err = validate_transition(
            &registry,
            "autopilot",
            KnotType::Gate,
            READY_TO_EVALUATE,
            "shipped",
            false,
        )
        .unwrap_err();
        assert!(err.to_string().contains("invalid gate transition"));
    }

    #[test]
    fn gate_owner_and_next_state_return_none_for_terminal_states() {
        let registry = ProfileRegistry::load().unwrap();
        let gate = GateData::default();
        assert_eq!(
            owner_kind_for_state(&registry, "autopilot", KnotType::Gate, &gate, "shipped").unwrap(),
            None
        );
        assert_eq!(
            next_happy_path_state(&registry, "autopilot", KnotType::Gate, "abandoned").unwrap(),
            None
        );
    }

    #[test]
    fn gate_transition_allows_noop_force_and_abandon() {
        let registry = ProfileRegistry::load().unwrap();
        assert!(validate_transition(
            &registry,
            "autopilot",
            KnotType::Gate,
            READY_TO_EVALUATE,
            READY_TO_EVALUATE,
            false
        )
        .is_ok());
        assert!(validate_transition(
            &registry,
            "autopilot",
            KnotType::Gate,
            READY_TO_EVALUATE,
            "abandoned",
            false
        )
        .is_ok());
        assert!(validate_transition(
            &registry,
            "autopilot",
            KnotType::Gate,
            READY_TO_EVALUATE,
            "shipped",
            true
        )
        .is_ok());
    }

    #[test]
    fn lease_initial_state_is_lease_ready() {
        let registry = ProfileRegistry::load().unwrap();
        let profile = registry.require("autopilot").unwrap();
        assert_eq!(initial_state(KnotType::Lease, profile), super::LEASE_READY);
    }

    #[test]
    fn lease_next_happy_path_follows_lifecycle() {
        let registry = ProfileRegistry::load().unwrap();
        assert_eq!(
            next_happy_path_state(&registry, "autopilot", KnotType::Lease, super::LEASE_READY,)
                .unwrap(),
            Some(super::LEASE_ACTIVE.to_string())
        );
        assert_eq!(
            next_happy_path_state(&registry, "autopilot", KnotType::Lease, super::LEASE_ACTIVE,)
                .unwrap(),
            Some(super::LEASE_TERMINATED.to_string())
        );
        assert_eq!(
            next_happy_path_state(
                &registry,
                "autopilot",
                KnotType::Lease,
                super::LEASE_TERMINATED,
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn lease_terminal_state_is_terminated() {
        let registry = ProfileRegistry::load().unwrap();
        assert!(is_terminal_state(
            &registry,
            "autopilot",
            KnotType::Lease,
            super::LEASE_TERMINATED,
        )
        .unwrap());
        assert!(
            !is_terminal_state(&registry, "autopilot", KnotType::Lease, super::LEASE_ACTIVE,)
                .unwrap()
        );
    }

    #[test]
    fn lease_owner_kind_is_always_none() {
        let registry = ProfileRegistry::load().unwrap();
        let gate = GateData::default();
        assert_eq!(
            owner_kind_for_state(
                &registry,
                "autopilot",
                KnotType::Lease,
                &gate,
                super::LEASE_ACTIVE,
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn lease_transition_rules() {
        let registry = ProfileRegistry::load().unwrap();
        // Valid transitions
        assert!(validate_transition(
            &registry,
            "autopilot",
            KnotType::Lease,
            super::LEASE_READY,
            super::LEASE_ACTIVE,
            false,
        )
        .is_ok());
        assert!(validate_transition(
            &registry,
            "autopilot",
            KnotType::Lease,
            super::LEASE_ACTIVE,
            super::LEASE_TERMINATED,
            false,
        )
        .is_ok());
        // Direct ready -> terminated
        assert!(validate_transition(
            &registry,
            "autopilot",
            KnotType::Lease,
            super::LEASE_READY,
            super::LEASE_TERMINATED,
            false,
        )
        .is_ok());
        // Noop (same state)
        assert!(validate_transition(
            &registry,
            "autopilot",
            KnotType::Lease,
            super::LEASE_ACTIVE,
            super::LEASE_ACTIVE,
            false,
        )
        .is_ok());
        // Invalid (terminated -> active)
        let err = validate_transition(
            &registry,
            "autopilot",
            KnotType::Lease,
            super::LEASE_TERMINATED,
            super::LEASE_ACTIVE,
            false,
        )
        .unwrap_err();
        assert!(err.to_string().contains("invalid lease transition"));
        // Force overrides invalid
        assert!(validate_transition(
            &registry,
            "autopilot",
            KnotType::Lease,
            super::LEASE_TERMINATED,
            super::LEASE_ACTIVE,
            true,
        )
        .is_ok());
    }

    #[test]
    fn work_runtime_delegates_to_profile_definition() {
        let registry = ProfileRegistry::load().unwrap();
        let gate = GateData::default();
        assert_eq!(
            initial_state(KnotType::Work, registry.require("autopilot").unwrap()),
            "ready_for_planning"
        );
        assert!(is_queue_state_for_profile(
            &registry,
            "autopilot",
            KnotType::Work,
            "ready_for_planning",
        )
        .unwrap());
        assert!(
            is_action_state_for_profile(&registry, "autopilot", KnotType::Work, "planning",)
                .unwrap()
        );
        assert_eq!(
            next_happy_path_state(&registry, "autopilot", KnotType::Work, "planning").unwrap(),
            Some("ready_for_plan_review".to_string())
        );
        assert_eq!(
            owner_kind_for_state(
                &registry,
                "autopilot",
                KnotType::Work,
                &gate,
                "implementation"
            )
            .unwrap(),
            Some(OwnerKind::Agent)
        );
    }

    #[test]
    fn queue_and_action_checks_report_unknown_profiles() {
        let registry = ProfileRegistry::load().unwrap();
        let err = is_queue_state_for_profile(&registry, "missing", KnotType::Work, "planning")
            .expect_err("missing profile should fail");
        assert!(err.to_string().contains("unknown profile"));
        let err = is_action_state_for_profile(&registry, "missing", KnotType::Work, "planning")
            .expect_err("missing profile should fail");
        assert!(err.to_string().contains("unknown profile"));
    }

    #[test]
    fn gate_and_lease_queue_action_helpers_cover_remaining_paths() {
        let registry = ProfileRegistry::load().unwrap();
        assert!(is_queue_state_for_profile(
            &registry,
            "autopilot",
            KnotType::Gate,
            READY_TO_EVALUATE,
        )
        .unwrap());
        assert!(is_queue_state_for_profile(
            &registry,
            "autopilot",
            KnotType::Lease,
            super::LEASE_READY,
        )
        .unwrap());
        assert!(
            is_action_state_for_profile(&registry, "autopilot", KnotType::Gate, EVALUATING,)
                .unwrap()
        );
        assert!(is_action_state_for_profile(
            &registry,
            "autopilot",
            KnotType::Lease,
            super::LEASE_ACTIVE,
        )
        .unwrap());
        assert_eq!(queue_state_for_stage("unknown-stage"), None);
    }

    #[test]
    fn custom_workflow_failure_outcomes_resolve_from_installed_bundle() {
        let root = unique_workspace("knots-runtime-outcomes");
        let bundle = root.join("bundle.toml");
        std::fs::write(
            &bundle,
            r#"
[workflow]
name = "custom_flow"
version = 1
default_profile = "autopilot"

[states.ready_for_work]
display_name = "Ready"
kind = "queue"

[states.work]
display_name = "Work"
kind = "action"
action_type = "produce"
executor = "agent"
prompt = "work"

[states.ready_for_review]
display_name = "Review Queue"
kind = "queue"

[states.review]
display_name = "Review"
kind = "action"
action_type = "gate"
executor = "human"
prompt = "review"

[states.deferred]
display_name = "Deferred"
kind = "escape"

[states.done]
display_name = "Done"
kind = "terminal"

[steps.work_step]
queue = "ready_for_work"
action = "work"

[steps.review_step]
queue = "ready_for_review"
action = "review"

[phases.main]
produce = "work_step"
gate = "review_step"

[profiles.autopilot]
phases = ["main"]
output = "remote_main"

[prompts.work]
body = "Do work"

[prompts.work.success]
complete = "ready_for_review"

[prompts.work.failure]
blocked = "deferred"

[prompts.review]
body = "Review work"

[prompts.review.success]
approved = "done"

[prompts.review.failure]
changes = "ready_for_work"
"#,
        )
        .expect("bundle should write");
        installed_workflows::install_bundle(&root, &bundle).expect("bundle should install");
        installed_workflows::set_current_workflow_selection(&root, "custom_flow", Some(1), None)
            .expect("workflow selection should succeed");
        let registry = ProfileRegistry::load_for_repo(&root).expect("registry should load");

        assert_eq!(
            next_outcome_state(
                &registry,
                &root,
                "custom_flow",
                "custom_flow/autopilot",
                KnotType::Work,
                "work",
                "blocked",
            )
            .expect("outcome should resolve"),
            Some("deferred".to_string())
        );
        assert_eq!(
            next_outcome_state(
                &registry,
                &root,
                "custom_flow",
                "custom_flow/autopilot",
                KnotType::Work,
                "work",
                "success",
            )
            .expect("happy path should resolve"),
            Some("ready_for_review".to_string())
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn non_work_outcomes_return_none_without_installed_workflow_lookup() {
        let registry = ProfileRegistry::load().unwrap();
        let root = unique_workspace("knots-runtime-non-work-outcome");
        assert_eq!(
            next_outcome_state(
                &registry,
                &root,
                "custom_flow",
                "autopilot",
                KnotType::Gate,
                READY_TO_EVALUATE,
                "blocked",
            )
            .expect("gate outcomes should not error"),
            None
        );

        let _ = std::fs::remove_dir_all(root);
    }
}
