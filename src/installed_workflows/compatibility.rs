use std::collections::BTreeMap;

use crate::profile::{ProfileDefinition, ProfileError};

use super::{render_prompt_body, PromptDefinition, WorkflowDefinition, COMPATIBILITY_WORKFLOW_ID};

pub(super) fn compatibility_workflow() -> Result<WorkflowDefinition, ProfileError> {
    let builtin = crate::workflow::ProfileRegistry::load()?;
    let mut profiles = BTreeMap::new();
    for mut profile in builtin.list() {
        fill_compatibility_states(&mut profile);
        profiles.insert(profile.id.clone(), profile);
    }
    let workflow_id = COMPATIBILITY_WORKFLOW_ID.to_string();
    let (prompts, action_prompts) = build_compatibility_prompts();
    populate_profile_prompts(&workflow_id, &prompts, &mut profiles);
    Ok(WorkflowDefinition {
        id: workflow_id,
        version: 1,
        description: Some("Built-in Knots compatibility workflow".to_string()),
        default_profile: Some("autopilot".to_string()),
        builtin: true,
        profiles,
        prompts,
        action_prompts,
    })
}

fn populate_profile_prompts(
    workflow_id: &str,
    prompts: &BTreeMap<String, PromptDefinition>,
    profiles: &mut BTreeMap<String, ProfileDefinition>,
) {
    for profile in profiles.values_mut() {
        profile.action_prompts.clear();
        profile.prompt_acceptance.clear();
        for prompt in prompts.values() {
            let rendered = render_prompt_body(workflow_id, profile, prompt);
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
}

fn fill_compatibility_states(profile: &mut ProfileDefinition) {
    if profile.queue_states.is_empty() {
        profile.queue_states = profile
            .states
            .iter()
            .filter(|s| s.starts_with("ready_for_"))
            .cloned()
            .collect();
    }
    if profile.action_states.is_empty() {
        profile.action_states = profile
            .states
            .iter()
            .filter(|s| !profile.queue_states.iter().any(|q| q == *s))
            .filter(|s| !profile.terminal_states.iter().any(|t| t == *s))
            .filter(|s| !profile.escape_states.iter().any(|e| e == *s))
            .cloned()
            .collect();
    }
}

fn build_compatibility_prompts() -> (BTreeMap<String, PromptDefinition>, BTreeMap<String, String>) {
    let mut prompts = BTreeMap::new();
    let mut action_prompts = BTreeMap::new();
    for state in [
        "planning",
        "plan_review",
        "implementation",
        "implementation_review",
        "shipment",
        "shipment_review",
        "evaluating",
    ] {
        let Some(body) = crate::skills::skill_for_state(state) else {
            continue;
        };
        let name = state.to_string();
        prompts.insert(
            name.clone(),
            PromptDefinition {
                prompt_name: name.clone(),
                action_state: state.to_string(),
                accept: Vec::new(),
                success_target: None,
                failure_targets: Vec::new(),
                params: Vec::new(),
                body: body.to_string(),
            },
        );
        action_prompts.insert(state.to_string(), name);
    }
    (prompts, action_prompts)
}
