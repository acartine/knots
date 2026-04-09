use crate::domain::knot_type::KnotType;
use crate::profile::ProfileError;

use super::{render_prompt_body, BundleFormat, WorkflowDefinition, WorkflowRef};

/// Test-visible wrapper to build the bundled work workflow.
#[cfg(test)]
pub fn work_sdlc_workflow_for_test() -> Result<WorkflowDefinition, ProfileError> {
    work_sdlc_workflow()
}

/// Test-visible wrapper to build the bundled gate workflow.
#[cfg(test)]
#[allow(dead_code)]
pub fn gate_sdlc_workflow_for_test() -> Result<WorkflowDefinition, ProfileError> {
    gate_sdlc_workflow()
}

/// Test-visible wrapper to build the bundled lease workflow.
#[cfg(test)]
#[allow(dead_code)]
pub fn lease_sdlc_workflow_for_test() -> Result<WorkflowDefinition, ProfileError> {
    lease_sdlc_workflow()
}

/// Test-visible wrapper to build the bundled explore workflow.
#[cfg(test)]
#[allow(dead_code)]
pub fn explore_sdlc_workflow_for_test() -> Result<WorkflowDefinition, ProfileError> {
    explore_sdlc_workflow()
}

pub fn builtin_workflows() -> Result<Vec<(KnotType, WorkflowDefinition)>, ProfileError> {
    KnotType::ALL
        .into_iter()
        .map(|knot_type| Ok((knot_type, builtin_workflow(knot_type)?)))
        .collect::<Result<Vec<_>, ProfileError>>()
}

pub fn builtin_workflow_ref(knot_type: KnotType) -> WorkflowRef {
    let workflow = builtin_workflow(knot_type).expect("embedded builtin workflow should parse");
    WorkflowRef::new(workflow.id, Some(workflow.version))
}

pub(super) fn work_sdlc_workflow() -> Result<WorkflowDefinition, ProfileError> {
    build_builtin_workflow(
        crate::loom_work_bundle::BUNDLE_JSON,
        "Built-in work workflow",
        Some("autopilot"),
    )
}

pub(super) fn gate_sdlc_workflow() -> Result<WorkflowDefinition, ProfileError> {
    build_builtin_workflow(
        crate::loom_gate_bundle::BUNDLE_JSON,
        "Built-in gate workflow",
        Some("evaluate"),
    )
}

pub(super) fn lease_sdlc_workflow() -> Result<WorkflowDefinition, ProfileError> {
    build_builtin_workflow(
        crate::loom_lease_bundle::BUNDLE_JSON,
        "Built-in lease workflow",
        Some("lease"),
    )
}

pub(super) fn explore_sdlc_workflow() -> Result<WorkflowDefinition, ProfileError> {
    build_builtin_workflow(
        crate::loom_explore_bundle::BUNDLE_JSON,
        "Built-in explore workflow",
        Some("explore"),
    )
}

fn builtin_workflow(knot_type: KnotType) -> Result<WorkflowDefinition, ProfileError> {
    match knot_type {
        KnotType::Work => work_sdlc_workflow(),
        KnotType::Gate => gate_sdlc_workflow(),
        KnotType::Lease => lease_sdlc_workflow(),
        KnotType::Explore => explore_sdlc_workflow(),
    }
}

fn build_builtin_workflow(
    bundle_json: &str,
    description: &str,
    default_profile: Option<&str>,
) -> Result<WorkflowDefinition, ProfileError> {
    let mut workflow = super::parse_bundle(bundle_json, BundleFormat::Json)?;
    workflow.builtin = true;
    workflow.description = Some(description.to_string());
    if workflow.default_profile.is_none() {
        workflow.default_profile = default_profile.map(str::to_string);
    }

    for profile in workflow.profiles.values_mut() {
        profile.workflow_id = workflow.id.clone();
    }

    for prompt in workflow.prompts.values_mut() {
        if prompt.action_state.is_empty() {
            prompt.action_state = prompt.prompt_name.clone();
            workflow
                .action_prompts
                .insert(prompt.prompt_name.clone(), prompt.prompt_name.clone());
        }
    }

    let prompts = workflow.prompts.clone();
    for profile in workflow.profiles.values_mut() {
        profile.action_prompts.clear();
        profile.prompt_acceptance.clear();
        for prompt in prompts.values() {
            let rendered = render_prompt_body(&workflow.id, profile, prompt);
            profile
                .action_prompts
                .insert(prompt.action_state.clone(), rendered);
            if !prompt.accept.is_empty() {
                profile
                    .prompt_acceptance
                    .insert(prompt.action_state.clone(), prompt.accept.clone());
            }
        }
    }

    Ok(workflow)
}

#[cfg(test)]
mod tests {
    use super::build_builtin_workflow;

    const MINIMAL_BUNDLE_NO_DEFAULT: &str = r#"{
  "format": "knots-bundle",
  "format_version": 2,
  "workflow": {"name": "sample_builtin", "version": 1, "default_profile": null},
  "states": [
    {"id": "ready", "kind": "queue", "prompt": null},
    {"id": "work", "kind": "action", "prompt": "work_prompt", "executor": "agent", "output": "branch"},
    {"id": "done", "kind": "terminal", "prompt": null}
  ],
  "steps": [{"id": "work_step", "queue": "ready", "action": "work"}],
  "phases": [{"id": "main", "produce_step": "work_step"}],
  "profiles": [{"id": "autopilot", "description": null, "display_name": null, "phases": ["main"]}],
  "prompts": [
    {"name": "work_prompt", "accept": ["Ship it"], "body": "Do {{ output }}.", "outcomes": [{"name": "complete", "target": "done", "is_success": true}]},
    {"name": "evaluate", "accept": ["Check it"], "body": "Evaluate it.", "outcomes": [{"name": "complete", "target": "done", "is_success": true}]}
  ]
}"#;

    #[test]
    fn build_builtin_workflow_fills_missing_default_and_unphased_prompt_action_state() {
        let workflow = build_builtin_workflow(
            MINIMAL_BUNDLE_NO_DEFAULT,
            "Synthetic builtin",
            Some("autopilot"),
        )
        .expect("bundle should parse");

        assert_eq!(workflow.description.as_deref(), Some("Synthetic builtin"));
        assert_eq!(workflow.default_profile.as_deref(), Some("autopilot"));
        assert!(workflow.builtin);
        assert_eq!(
            workflow
                .require_profile("autopilot")
                .expect("profile")
                .workflow_id,
            "sample_builtin"
        );
        assert_eq!(
            workflow
                .prompts
                .get("evaluate")
                .expect("prompt")
                .action_state,
            "evaluate"
        );
        assert_eq!(
            workflow.action_prompts.get("evaluate").map(String::as_str),
            Some("evaluate")
        );
    }

    #[test]
    fn build_builtin_workflow_preserves_bundle_default_profile() {
        let bundle = MINIMAL_BUNDLE_NO_DEFAULT.replace(
            "\"default_profile\": null",
            "\"default_profile\": \"bundle_default\"",
        );
        let workflow = build_builtin_workflow(&bundle, "Synthetic builtin", Some("autopilot"))
            .expect("bundle should parse");
        assert_eq!(workflow.default_profile.as_deref(), Some("bundle_default"));
    }
}
