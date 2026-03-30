use std::collections::BTreeMap;

use serde::Deserialize;

use crate::profile::{normalize_profile_id, OwnerKind, ProfileError, StepOwner};

use super::bundle_json::{
    JsonKnotsBundle, JsonOutputEntry, JsonPhaseSection, JsonProfileSection, JsonPromptOutcome,
    JsonPromptParamSection, JsonPromptSection, JsonStateSection, JsonStepSection,
    JsonWorkflowSection,
};
use super::profile_toml::build_profile_definition;
use super::{PromptDefinition, PromptParamDefinition, WorkflowDefinition};

#[derive(Debug, Deserialize)]
pub(crate) struct BundleToml {
    pub workflow: BundleWorkflowSection,
    #[serde(default)]
    pub states: BTreeMap<String, BundleStateSection>,
    #[serde(default)]
    pub steps: BTreeMap<String, BundleStepSection>,
    #[serde(default)]
    pub phases: BTreeMap<String, BundlePhaseSection>,
    #[serde(default)]
    pub profiles: BTreeMap<String, BundleProfileSection>,
    #[serde(default)]
    pub prompts: BTreeMap<String, BundlePromptSection>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BundleWorkflowSection {
    pub name: String,
    pub version: u32,
    #[serde(default)]
    pub default_profile: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BundleStateSection {
    pub kind: String,
    #[serde(default)]
    pub executor: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub output_hint: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BundleStepSection {
    pub queue: String,
    pub action: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BundlePhaseSection {
    pub produce: String,
    pub gate: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BundleProfileSection {
    #[serde(default)]
    pub description: Option<String>,
    pub phases: Vec<String>,
    #[serde(default)]
    pub outputs: BTreeMap<String, BundleOutputEntry>,
    #[serde(default)]
    pub overrides: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BundleOutputEntry {
    pub artifact_type: String,
    #[serde(default)]
    pub access_hint: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BundlePromptSection {
    #[serde(default)]
    pub accept: Vec<String>,
    #[serde(default)]
    pub success: BTreeMap<String, String>,
    #[serde(default)]
    pub failure: BTreeMap<String, String>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub params: BTreeMap<String, BundlePromptParamSection>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BundlePromptParamSection {
    #[serde(rename = "type")]
    pub param_type: String,
    #[serde(default)]
    pub values: Vec<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

pub(crate) fn render_json_bundle_from_toml(raw: &str) -> Result<String, ProfileError> {
    let parsed: BundleToml =
        toml::from_str(raw).map_err(|err| ProfileError::InvalidBundle(err.to_string()))?;
    let bundle = build_json_bundle(parsed);
    serde_json::to_string_pretty(&bundle)
        .map_err(|err| ProfileError::InvalidBundle(err.to_string()))
}

fn build_json_bundle(parsed: BundleToml) -> JsonKnotsBundle {
    JsonKnotsBundle {
        format: "knots-bundle".to_string(),
        format_version: 2,
        workflow: JsonWorkflowSection {
            name: parsed.workflow.name,
            version: parsed.workflow.version,
            default_profile: parsed.workflow.default_profile,
        },
        states: parsed
            .states
            .into_iter()
            .map(|(id, state)| JsonStateSection {
                id,
                kind: state.kind,
                prompt: state.prompt,
                output: state.output,
                output_hint: state.output_hint,
            })
            .collect(),
        steps: parsed
            .steps
            .into_iter()
            .map(|(id, step)| JsonStepSection {
                id,
                queue: step.queue,
                action: step.action,
            })
            .collect(),
        phases: parsed
            .phases
            .into_iter()
            .map(|(id, phase)| JsonPhaseSection {
                id,
                produce_step: phase.produce,
                gate_step: phase.gate,
            })
            .collect(),
        profiles: build_json_profiles(parsed.profiles),
        prompts: build_json_prompts(parsed.prompts),
    }
}

fn build_json_profiles(
    profiles: BTreeMap<String, BundleProfileSection>,
) -> Vec<JsonProfileSection> {
    profiles
        .into_iter()
        .map(|(id, profile)| JsonProfileSection {
            id,
            description: profile.description,
            display_name: None,
            phases: profile.phases,
            outputs: profile
                .outputs
                .into_iter()
                .map(|(k, v)| {
                    (
                        k,
                        JsonOutputEntry {
                            artifact_type: v.artifact_type,
                            access_hint: v.access_hint,
                        },
                    )
                })
                .collect(),
            executors: profile.overrides,
        })
        .collect()
}

fn build_json_prompts(prompts: BTreeMap<String, BundlePromptSection>) -> Vec<JsonPromptSection> {
    prompts
        .into_iter()
        .map(|(name, prompt)| {
            let mut outcomes = prompt
                .success
                .into_iter()
                .map(|(name, target)| JsonPromptOutcome {
                    name,
                    target,
                    is_success: true,
                })
                .collect::<Vec<_>>();
            outcomes.extend(
                prompt
                    .failure
                    .into_iter()
                    .map(|(name, target)| JsonPromptOutcome {
                        name,
                        target,
                        is_success: false,
                    }),
            );
            JsonPromptSection {
                name,
                accept: prompt.accept,
                body: prompt.body,
                params: prompt
                    .params
                    .into_iter()
                    .map(|(name, value)| JsonPromptParamSection {
                        name,
                        param_type: value.param_type,
                        values: value.values,
                        required: value.required,
                        default: value.default,
                        description: value.description,
                    })
                    .collect(),
                outcomes,
            }
        })
        .collect()
}

pub(crate) fn parse_bundle_toml(raw: &str) -> Result<WorkflowDefinition, ProfileError> {
    let parsed: BundleToml =
        toml::from_str(raw).map_err(|err| ProfileError::InvalidBundle(err.to_string()))?;
    let workflow_id = normalize_profile_id(&parsed.workflow.name)
        .ok_or_else(|| ProfileError::InvalidBundle("workflow.name is required".to_string()))?;

    let mut prompts = parse_toml_prompts(&parsed.prompts)?;
    let mut profiles = BTreeMap::new();
    let mut action_prompts = BTreeMap::new();

    for (profile_name, profile_section) in &parsed.profiles {
        let profile = build_profile_definition(
            &workflow_id,
            profile_name,
            profile_section,
            &parsed.states,
            &parsed.steps,
            &parsed.phases,
            &parsed.prompts,
        )?;
        collect_action_prompts(&profile, &parsed.states, profile_name, &mut action_prompts)?;
        profiles.insert(profile.id.clone(), profile);
    }

    for (action_state, prompt_name) in &action_prompts {
        if let Some(prompt) = prompts.get_mut(prompt_name) {
            prompt.action_state = action_state.clone();
        }
    }

    Ok(WorkflowDefinition {
        id: workflow_id,
        version: parsed.workflow.version,
        description: None,
        default_profile: parsed
            .workflow
            .default_profile
            .as_deref()
            .and_then(normalize_profile_id),
        builtin: false,
        profiles,
        prompts,
        action_prompts,
    })
}

fn parse_toml_prompts(
    prompts: &BTreeMap<String, BundlePromptSection>,
) -> Result<BTreeMap<String, PromptDefinition>, ProfileError> {
    let mut result = BTreeMap::new();
    for (prompt_name, prompt) in prompts {
        let success_target = match prompt.success.len() {
            0 => None,
            1 => Some(prompt.success.values().next().cloned().unwrap_or_default()),
            _ => {
                return Err(ProfileError::InvalidBundle(format!(
                    "prompt '{}' has multiple success \
                     outcomes; Knots requires one \
                     happy-path target",
                    prompt_name
                )))
            }
        };
        let params = prompt
            .params
            .iter()
            .map(|(name, value)| PromptParamDefinition {
                name: name.clone(),
                param_type: value.param_type.clone(),
                values: value.values.clone(),
                required: value.required,
                default: value.default.clone(),
                description: value.description.clone(),
            })
            .collect::<Vec<_>>();
        result.insert(
            prompt_name.clone(),
            PromptDefinition {
                prompt_name: prompt_name.clone(),
                action_state: String::new(),
                accept: prompt.accept.clone(),
                success_target,
                failure_targets: prompt
                    .failure
                    .iter()
                    .map(|(o, t)| (o.clone(), t.clone()))
                    .collect(),
                params,
                body: prompt.body.clone(),
            },
        );
    }
    Ok(result)
}

fn collect_action_prompts(
    profile: &crate::profile::ProfileDefinition,
    states: &BTreeMap<String, BundleStateSection>,
    profile_name: &str,
    action_prompts: &mut BTreeMap<String, String>,
) -> Result<(), ProfileError> {
    for action_state in &profile.action_states {
        let state = states.get(action_state).ok_or_else(|| {
            ProfileError::InvalidBundle(format!(
                "profile '{}' references missing state '{}'",
                profile_name, action_state
            ))
        })?;
        let prompt_name = state.prompt.as_ref().ok_or_else(|| {
            ProfileError::InvalidBundle(format!(
                "action '{}' is missing prompt metadata",
                action_state
            ))
        })?;
        action_prompts.insert(action_state.clone(), prompt_name.clone());
    }
    Ok(())
}

pub(super) fn default_owner(kind: OwnerKind) -> StepOwner {
    StepOwner {
        kind,
        agent_name: None,
        agent_model: None,
        agent_version: None,
    }
}
