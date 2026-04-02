use crate::profile::ProfileError;

use super::{render_prompt_body, BundleFormat, WorkflowDefinition, COMPATIBILITY_WORKFLOW_ID};

pub(super) fn compatibility_workflow() -> Result<WorkflowDefinition, ProfileError> {
    let mut workflow =
        super::parse_bundle(crate::loom_compat_bundle::BUNDLE_JSON, BundleFormat::Json)?;

    let workflow_id = COMPATIBILITY_WORKFLOW_ID.to_string();
    workflow.id = workflow_id.clone();
    workflow.builtin = true;
    workflow.description = Some("Built-in Knots compatibility workflow".to_string());
    if workflow.default_profile.is_none() {
        workflow.default_profile = Some("autopilot".to_string());
    }

    for profile in workflow.profiles.values_mut() {
        profile.workflow_id = workflow_id.clone();
    }

    // Prompts for actions not reachable through any profile phase
    // (e.g. evaluating) have an empty action_state after bundle
    // parsing. Fix them by inferring the state from the prompt name.
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
            let step_metadata = profile.step_metadata_for(&prompt.action_state);
            let rendered = render_prompt_body(
                &workflow_id,
                &profile.id,
                step_metadata.output.as_ref(),
                prompt,
            );
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
