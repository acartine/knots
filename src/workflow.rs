#![allow(unused_imports)]

pub use crate::profile::{
    normalize_profile_id, normalize_profile_id as normalize_workflow_id, GateMode,
    InvalidWorkflowTransition, OutputMode, OwnerKind, ProfileDefinition,
    ProfileDefinition as WorkflowDefinition, ProfileError, ProfileError as WorkflowError,
    ProfileOwners, ProfileRegistry, ProfileRegistry as WorkflowRegistry, StepOwner,
    WorkflowTransition,
};

#[cfg(test)]
mod tests {
    use super::WorkflowRegistry;

    #[test]
    fn loads_embedded_default_workflow() {
        let registry = WorkflowRegistry::load().expect("embedded registry should load");
        assert!(registry.require("autopilot").is_ok());
        assert!(registry.require("autopilot_with_pr").is_ok());
        assert!(registry.require("semiauto").is_ok());
    }

    #[test]
    fn unknown_workflow_error_is_descriptive() {
        let err = WorkflowRegistry::load()
            .expect("registry should load")
            .require("missing")
            .expect_err("unknown workflow should fail");
        assert!(format!("{err}").contains("unknown profile"));
    }
}

#[cfg(test)]
#[path = "workflow_tests_ext.rs"]
mod tests_ext;
