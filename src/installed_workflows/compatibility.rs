use std::collections::BTreeMap;

use crate::profile::{ActionOutputDef, ProfileDefinition, ProfileError};

use super::{render_prompt_body, BundleFormat, WorkflowDefinition, COMPATIBILITY_WORKFLOW_ID};

pub(super) fn compatibility_workflow() -> Result<WorkflowDefinition, ProfileError> {
    let mut workflow = super::parse_bundle(
        crate::loom_compat_bundle::builtin_bundle_json(),
        BundleFormat::Json,
    )?;
    let workflow_id = COMPATIBILITY_WORKFLOW_ID.to_string();
    workflow.id = workflow_id.clone();
    workflow.version = 1;
    workflow.description = Some("Built-in Knots compatibility workflow".to_string());
    workflow.default_profile = Some("autopilot".to_string());
    workflow.builtin = true;
    let prompts = workflow.prompts.values().cloned().collect::<Vec<_>>();

    for profile in workflow.profiles.values_mut() {
        profile.workflow_id = workflow_id.clone();
        normalize_compatibility_outputs(profile);
        profile.action_prompts.clear();
        profile.prompt_acceptance.clear();
        for prompt in &prompts {
            let rendered = render_prompt_body(&workflow_id, profile, prompt);
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

fn normalize_compatibility_outputs(profile: &mut ProfileDefinition) {
    let artifact_type = compatibility_output_mode(profile);
    profile.outputs = profile
        .action_states
        .iter()
        .cloned()
        .map(|state| {
            (
                state,
                ActionOutputDef {
                    artifact_type: artifact_type.to_string(),
                    access_hint: None,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
}

fn compatibility_output_mode(profile: &ProfileDefinition) -> &'static str {
    if profile
        .outputs
        .values()
        .any(|output| output.artifact_type == "pr")
    {
        "pr"
    } else {
        "remote_main"
    }
}
