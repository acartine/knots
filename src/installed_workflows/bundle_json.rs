use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::profile::{normalize_profile_id, ProfileError};

use super::profile_json::{build_json_profile, BundleIndexes};
use super::{PromptDefinition, PromptParamDefinition, WorkflowDefinition};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct JsonKnotsBundle {
    pub format: String,
    pub format_version: u32,
    pub workflow: JsonWorkflowSection,
    pub states: Vec<JsonStateSection>,
    pub steps: Vec<JsonStepSection>,
    pub phases: Vec<JsonPhaseSection>,
    pub profiles: Vec<JsonProfileSection>,
    pub prompts: Vec<JsonPromptSection>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct JsonWorkflowSection {
    pub name: String,
    pub version: u32,
    pub default_profile: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct JsonStateSection {
    pub id: String,
    pub kind: String,
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_hint: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct JsonPhaseSection {
    pub id: String,
    pub produce_step: String,
    #[serde(default)]
    pub gate_step: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct JsonStepSection {
    pub id: String,
    pub queue: String,
    pub action: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct JsonProfileSection {
    pub id: String,
    pub description: Option<String>,
    pub display_name: Option<String>,
    pub phases: Vec<String>,
    #[serde(default)]
    pub outputs: BTreeMap<String, JsonOutputEntry>,
    #[serde(default)]
    pub executors: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct JsonOutputEntry {
    pub artifact_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_hint: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct JsonPromptSection {
    pub name: String,
    #[serde(default)]
    pub accept: Vec<String>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub params: Vec<JsonPromptParamSection>,
    #[serde(default)]
    pub outcomes: Vec<JsonPromptOutcome>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct JsonPromptOutcome {
    #[serde(default)]
    pub name: String,
    pub target: String,
    pub is_success: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct JsonPromptParamSection {
    pub name: String,
    #[serde(alias = "type", alias = "param_type", rename = "type")]
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

pub(crate) fn parse_bundle_json(raw: &str) -> Result<WorkflowDefinition, ProfileError> {
    let parsed: JsonKnotsBundle =
        serde_json::from_str(raw).map_err(|err| ProfileError::InvalidBundle(err.to_string()))?;
    validate_bundle_metadata(&parsed)?;

    let workflow_id = normalize_profile_id(&parsed.workflow.name)
        .ok_or_else(|| ProfileError::InvalidBundle("workflow.name is required".to_string()))?;
    let indexes = BundleIndexes::build(
        &parsed.states,
        &parsed.steps,
        &parsed.phases,
        &parsed.prompts,
    );
    let mut prompts = build_prompts(&parsed.prompts);

    let mut profiles = BTreeMap::new();
    let mut action_prompts = BTreeMap::new();
    for profile in &parsed.profiles {
        let (built, profile_actions) =
            build_json_profile(&workflow_id, profile, &indexes, &parsed.states)?;
        action_prompts.extend(profile_actions);
        profiles.insert(built.id.clone(), built);
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

fn validate_bundle_metadata(parsed: &JsonKnotsBundle) -> Result<(), ProfileError> {
    if parsed.format != "knots-bundle" {
        return Err(ProfileError::InvalidBundle(format!(
            "unsupported bundle format '{}'",
            parsed.format
        )));
    }
    if parsed.format_version != 1 && parsed.format_version != 2 {
        return Err(ProfileError::InvalidBundle(format!(
            "unsupported bundle format version '{}'",
            parsed.format_version
        )));
    }
    Ok(())
}

fn build_prompts(prompts: &[JsonPromptSection]) -> BTreeMap<String, PromptDefinition> {
    prompts
        .iter()
        .map(|prompt| {
            let success_target = prompt
                .outcomes
                .iter()
                .find(|o| o.is_success)
                .map(|o| o.target.clone());
            (
                prompt.name.clone(),
                PromptDefinition {
                    prompt_name: prompt.name.clone(),
                    action_state: String::new(),
                    accept: prompt.accept.clone(),
                    success_target,
                    failure_targets: prompt
                        .outcomes
                        .iter()
                        .filter(|o| !o.is_success)
                        .map(|o| {
                            (
                                if o.name.trim().is_empty() {
                                    o.target.clone()
                                } else {
                                    o.name.clone()
                                },
                                o.target.clone(),
                            )
                        })
                        .collect(),
                    params: prompt
                        .params
                        .iter()
                        .map(|p| PromptParamDefinition {
                            name: p.name.clone(),
                            param_type: p.param_type.clone(),
                            values: p.values.clone(),
                            required: p.required,
                            default: p.default.clone(),
                            description: p.description.clone(),
                        })
                        .collect(),
                    body: prompt.body.clone(),
                },
            )
        })
        .collect()
}
