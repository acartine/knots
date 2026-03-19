use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::profile::{
    normalize_profile_id, GateMode, OutputMode, OwnerKind, ProfileDefinition, ProfileError,
    ProfileOwners, StepOwner, WorkflowTransition,
};

pub const COMPATIBILITY_WORKFLOW_ID: &str = "compatibility";
const DEFAULT_BUNDLE_FILE: &str = "bundle.json";
const TOML_BUNDLE_FILE: &str = "bundle.toml";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowRepoConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_workflow: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptParamDefinition {
    pub name: String,
    pub param_type: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptDefinition {
    pub prompt_name: String,
    pub action_state: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accept: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success_target: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failure_targets: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<PromptParamDefinition>,
    pub body: String,
}

#[cfg(test)]
impl PromptDefinition {
    pub fn render(&self, workflow: &WorkflowDefinition, profile: &ProfileDefinition) -> String {
        let params = build_prompt_params(workflow, profile, self);
        let mut unresolved = Vec::new();
        let mut body = render_prompt_template(&self.body, &params, &mut unresolved);
        if !self.accept.is_empty() {
            if !body.is_empty() {
                body.push_str("\n\n");
            }
            body.push_str("## Acceptance Criteria\n\n");
            for item in &self.accept {
                body.push_str(&format!("- {item}\n"));
            }
        }
        if !unresolved.is_empty() {
            body.push_str("\n\n## Unresolved Parameters\n\n");
            for name in unresolved {
                body.push_str(&format!("- {name}\n"));
            }
        }
        body
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowDefinition {
    pub id: String,
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<String>,
    #[serde(default)]
    pub builtin: bool,
    pub profiles: BTreeMap<String, ProfileDefinition>,
    pub prompts: BTreeMap<String, PromptDefinition>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub action_prompts: BTreeMap<String, String>,
}

impl WorkflowDefinition {
    pub fn require_profile(&self, profile_id: &str) -> Result<&ProfileDefinition, ProfileError> {
        let id = normalize_profile_id(profile_id)
            .ok_or_else(|| ProfileError::UnknownProfile(profile_id.to_string()))?;
        self.profiles
            .get(&id)
            .ok_or(ProfileError::UnknownProfile(id))
    }

    pub fn list_profiles(&self) -> Vec<ProfileDefinition> {
        self.profiles.values().cloned().collect()
    }

    #[allow(dead_code)]
    pub fn prompt_for_action_state(&self, state: &str) -> Option<&PromptDefinition> {
        let prompt_name = self.action_prompts.get(state)?;
        self.prompts.get(prompt_name)
    }

    pub fn display_description(&self) -> Option<&str> {
        self.description.as_deref()
    }
}

#[derive(Debug, Clone)]
pub struct InstalledWorkflowRegistry {
    workflows: BTreeMap<String, BTreeMap<u32, WorkflowDefinition>>,
    current: Option<WorkflowRepoConfig>,
}

impl InstalledWorkflowRegistry {
    pub fn load(repo_root: &Path) -> Result<Self, ProfileError> {
        let mut workflows: BTreeMap<String, BTreeMap<u32, WorkflowDefinition>> = BTreeMap::new();
        let compatibility = compatibility_workflow()?;
        workflows
            .entry(compatibility.id.clone())
            .or_default()
            .insert(compatibility.version, compatibility);

        let workflows_root = workflows_root(repo_root);
        if workflows_root.exists() {
            let mut workflow_entries = fs::read_dir(&workflows_root)
                .map_err(|err| ProfileError::InvalidBundle(err.to_string()))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|err| ProfileError::InvalidBundle(err.to_string()))?;
            workflow_entries.sort_by_key(|entry| entry.file_name());
            for workflow_entry in workflow_entries {
                let workflow_path = workflow_entry.path();
                if !workflow_path.is_dir() {
                    continue;
                }
                let workflow_id = workflow_entry.file_name().to_string_lossy().to_string();
                let mut version_entries = fs::read_dir(&workflow_path)
                    .map_err(|err| ProfileError::InvalidBundle(err.to_string()))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|err| ProfileError::InvalidBundle(err.to_string()))?;
                version_entries.sort_by_key(|entry| entry.file_name());
                for version_entry in version_entries {
                    let version_path = version_entry.path();
                    if !version_path.is_dir() {
                        continue;
                    }
                    let Some(version_name) = version_path.file_name().and_then(|name| name.to_str())
                    else {
                        continue;
                    };
                    let Ok(version) = version_name.parse::<u32>() else {
                        continue;
                    };
                    let Some(bundle_path) = installed_bundle_path(&version_path) else {
                        continue;
                    };
                    let raw = fs::read_to_string(&bundle_path)
                        .map_err(|err| ProfileError::InvalidBundle(err.to_string()))?;
                    let format = match bundle_path.extension().and_then(|ext| ext.to_str()) {
                        Some("json") => BundleFormat::Json,
                        _ => BundleFormat::Toml,
                    };
                    let workflow = parse_bundle(&raw, format)?;
                    workflows
                        .entry(workflow_id.clone())
                        .or_default()
                        .insert(version, workflow);
                }
            }
        }

        let current = read_repo_config(repo_root)?;

        Ok(Self {
            workflows,
            current: Some(current),
        })
    }

    pub fn current_workflow_id(&self) -> &str {
        self.current
            .as_ref()
            .and_then(|config| config.current_workflow.as_deref())
            .unwrap_or(COMPATIBILITY_WORKFLOW_ID)
    }

    pub fn current_workflow_version(&self) -> Option<u32> {
        self.current.as_ref().and_then(|config| config.current_version)
    }

    pub fn current_profile_id(&self) -> Option<&str> {
        self.current
            .as_ref()
            .and_then(|config| config.current_profile.as_deref())
    }

    pub fn current_workflow(&self) -> Result<&WorkflowDefinition, ProfileError> {
        if let Some(config) = self.current.as_ref() {
            if let Some(workflow_id) = config.current_workflow.as_deref() {
                if let Some(version) = config.current_version {
                    return self.require_workflow_version(workflow_id, version);
                }
                return self.require_workflow(workflow_id);
            }
        }
        self.require_workflow(COMPATIBILITY_WORKFLOW_ID)
    }

    pub fn require_workflow(&self, workflow_id: &str) -> Result<&WorkflowDefinition, ProfileError> {
        let workflow_id = normalize_profile_id(workflow_id)
            .ok_or_else(|| ProfileError::UnknownWorkflow(workflow_id.to_string()))?;
        self.workflows
            .get(&workflow_id)
            .and_then(|versions| versions.iter().next_back().map(|(_, workflow)| workflow))
            .ok_or(ProfileError::UnknownWorkflow(workflow_id))
    }

    pub fn require_workflow_version(
        &self,
        workflow_id: &str,
        version: u32,
    ) -> Result<&WorkflowDefinition, ProfileError> {
        let id = normalize_profile_id(workflow_id)
            .ok_or_else(|| ProfileError::UnknownWorkflow(workflow_id.to_string()))?;
        self.workflows
            .get(&id)
            .and_then(|versions| versions.get(&version))
            .ok_or(ProfileError::UnknownWorkflow(id))
    }

    pub fn list(&self) -> Vec<&WorkflowDefinition> {
        let mut result = Vec::new();
        for versions in self.workflows.values() {
            result.extend(versions.values());
        }
        result.sort_by(|left, right| {
            left.id
                .cmp(&right.id)
                .then_with(|| left.version.cmp(&right.version))
        });
        result
    }
}

pub fn repo_config_path(repo_root: &Path) -> PathBuf {
    workflows_root(repo_root).join("current")
}

pub fn workflows_root(repo_root: &Path) -> PathBuf {
    repo_root.join(".knots").join("workflows")
}

pub fn read_repo_config(repo_root: &Path) -> Result<WorkflowRepoConfig, ProfileError> {
    let path = repo_config_path(repo_root);
    if !path.exists() {
        return Ok(WorkflowRepoConfig::default());
    }
    let raw = fs::read_to_string(&path).map_err(|err| ProfileError::InvalidBundle(err.to_string()))?;
    toml::from_str(&raw).map_err(|err| ProfileError::InvalidBundle(err.to_string()))
}

pub fn write_repo_config(repo_root: &Path, config: &WorkflowRepoConfig) -> Result<(), ProfileError> {
    let path = repo_config_path(repo_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| ProfileError::InvalidBundle(err.to_string()))?;
    }
    let rendered =
        toml::to_string_pretty(config).map_err(|err| ProfileError::InvalidBundle(err.to_string()))?;
    fs::write(path, rendered).map_err(|err| ProfileError::InvalidBundle(err.to_string()))
}

pub fn install_bundle(repo_root: &Path, source: &Path) -> Result<String, ProfileError> {
    let (raw, format) = read_bundle_source(source)?;
    let workflow = parse_bundle(&raw, format)?;
    let canonical_json = match format {
        BundleFormat::Json => raw.clone(),
        BundleFormat::Toml => render_json_bundle_from_toml(&raw)?,
    };
    let workflow_dir = workflows_root(repo_root).join(&workflow.id);
    let target_dir = workflow_dir.join(workflow.version.to_string());
    fs::create_dir_all(&target_dir).map_err(|err| ProfileError::InvalidBundle(err.to_string()))?;
    fs::write(target_dir.join(DEFAULT_BUNDLE_FILE), &canonical_json)
        .map_err(|err| ProfileError::InvalidBundle(err.to_string()))?;
    fs::write(workflow_dir.join(DEFAULT_BUNDLE_FILE), &canonical_json)
        .map_err(|err| ProfileError::InvalidBundle(err.to_string()))?;
    if matches!(format, BundleFormat::Toml) {
        fs::write(target_dir.join(TOML_BUNDLE_FILE), &raw)
            .map_err(|err| ProfileError::InvalidBundle(err.to_string()))?;
        fs::write(workflow_dir.join(TOML_BUNDLE_FILE), &raw)
            .map_err(|err| ProfileError::InvalidBundle(err.to_string()))?;
    }
    let _ = set_current_workflow_selection(repo_root, &workflow.id, Some(workflow.version), None);
    Ok(workflow.id)
}

pub fn namespaced_profile_id(workflow_id: &str, profile_id: &str) -> String {
    format!("{workflow_id}/{profile_id}")
}

pub fn set_current_workflow_selection(
    repo_root: &Path,
    workflow_id: &str,
    version: Option<u32>,
    profile_id: Option<&str>,
) -> Result<WorkflowRepoConfig, ProfileError> {
    let registry = InstalledWorkflowRegistry::load(repo_root)?;
    let workflow = match version {
        Some(version) => registry.require_workflow_version(workflow_id, version)?,
        None => registry.require_workflow(workflow_id)?,
    };
    let selected_profile_id = match profile_id {
        Some(profile_id) => workflow.require_profile(profile_id)?.id.clone(),
        None => workflow
            .default_profile
            .as_deref()
            .and_then(|default_profile| workflow.require_profile(default_profile).ok())
            .map(|profile| profile.id.clone())
            .or_else(|| workflow.list_profiles().into_iter().next().map(|profile| profile.id.clone()))
            .unwrap_or_else(|| "autopilot".to_string()),
    };
    let selected_profile_id = if workflow.builtin {
        selected_profile_id
    } else {
        namespaced_profile_id(&workflow.id, &selected_profile_id)
    };
    let config = WorkflowRepoConfig {
        current_workflow: Some(workflow.id.clone()),
        current_version: Some(workflow.version),
        current_profile: Some(selected_profile_id),
    };
    write_repo_config(repo_root, &config)?;
    Ok(config)
}

fn read_bundle_source(source: &Path) -> Result<(String, BundleFormat), ProfileError> {
    if source.is_dir() && source.join("loom.toml").exists() {
        let output = Command::new("loom")
            .arg("build")
            .arg(source)
            .arg("--emit")
            .arg("knots-bundle")
            .output()
            .map_err(|err| ProfileError::InvalidBundle(format!("failed to execute loom: {err}")))?;
        if !output.status.success() {
            return Err(ProfileError::InvalidBundle(format!(
                "loom build --emit knots-bundle failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let raw = String::from_utf8(output.stdout)
            .map_err(|err| ProfileError::InvalidBundle(format!("invalid UTF-8 bundle output: {err}")))?;
        return Ok((raw, BundleFormat::Json));
    }

    let source_path = resolve_bundle_source_path(source)?;
    let raw =
        fs::read_to_string(&source_path).map_err(|err| ProfileError::InvalidBundle(err.to_string()))?;
    let format = match source_path.extension().and_then(|ext| ext.to_str()) {
        Some("json") => BundleFormat::Json,
        _ => BundleFormat::Toml,
    };
    Ok((raw, format))
}

fn resolve_bundle_source_path(source: &Path) -> Result<PathBuf, ProfileError> {
    if source.is_file() {
        return Ok(source.to_path_buf());
    }
    if !source.is_dir() {
        return Err(ProfileError::InvalidBundle(format!(
            "bundle source '{}' does not exist",
            source.display()
        )));
    }

    let candidates = [
        source.join(DEFAULT_BUNDLE_FILE),
        source.join(TOML_BUNDLE_FILE),
        source.join("workflow.json"),
        source.join("workflow.toml"),
        source.join("dist").join(DEFAULT_BUNDLE_FILE),
        source.join("dist").join(TOML_BUNDLE_FILE),
        source.join("build").join(DEFAULT_BUNDLE_FILE),
        source.join("build").join(TOML_BUNDLE_FILE),
    ];
    for candidate in candidates {
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(ProfileError::InvalidBundle(format!(
        "no Loom bundle found in '{}'; expected bundle.json, bundle.toml, workflow.json, or workflow.toml",
        source.display()
    )))
}

fn installed_bundle_path(workflow_dir: &Path) -> Option<PathBuf> {
    let bundle = workflow_dir.join(DEFAULT_BUNDLE_FILE);
    if bundle.exists() {
        Some(bundle)
    } else {
        let legacy = workflow_dir.join("bundle.toml");
        legacy.exists().then_some(legacy)
    }
}

fn compatibility_workflow() -> Result<WorkflowDefinition, ProfileError> {
    let builtin_registry = crate::workflow::ProfileRegistry::load()?;
    let mut profiles = BTreeMap::new();
    for mut profile in builtin_registry.list() {
        if profile.queue_states.is_empty() {
            profile.queue_states = profile
                .states
                .iter()
                .filter(|state| state.starts_with("ready_for_"))
                .cloned()
                .collect();
        }
        if profile.action_states.is_empty() {
            profile.action_states = profile
                .states
                .iter()
                .filter(|state| !profile.queue_states.iter().any(|queue| queue == *state))
                .filter(|state| !profile.terminal_states.iter().any(|terminal| terminal == *state))
                .filter(|state| *state != "deferred" && *state != "abandoned")
                .cloned()
                .collect();
        }
        profiles.insert(profile.id.clone(), profile);
    }

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
        let prompt_name = state.to_string();
        prompts.insert(
            prompt_name.clone(),
            PromptDefinition {
                prompt_name: prompt_name.clone(),
                action_state: state.to_string(),
                accept: Vec::new(),
                success_target: None,
                failure_targets: Vec::new(),
                params: Vec::new(),
                body: body.to_string(),
            },
        );
        action_prompts.insert(state.to_string(), prompt_name);
    }

    Ok(WorkflowDefinition {
        id: COMPATIBILITY_WORKFLOW_ID.to_string(),
        version: 1,
        description: Some("Built-in Knots compatibility workflow".to_string()),
        default_profile: Some("autopilot".to_string()),
        builtin: true,
        profiles,
        prompts,
        action_prompts,
    })
}

#[derive(Debug, Deserialize)]
struct BundleToml {
    workflow: BundleWorkflowSection,
    #[serde(default)]
    states: BTreeMap<String, BundleStateSection>,
    #[serde(default)]
    steps: BTreeMap<String, BundleStepSection>,
    #[serde(default)]
    phases: BTreeMap<String, BundlePhaseSection>,
    #[serde(default)]
    profiles: BTreeMap<String, BundleProfileSection>,
    #[serde(default)]
    prompts: BTreeMap<String, BundlePromptSection>,
}

#[derive(Debug, Deserialize)]
struct BundleWorkflowSection {
    name: String,
    version: u32,
    #[serde(default)]
    default_profile: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BundleStateSection {
    kind: String,
    #[serde(default)]
    executor: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BundleStepSection {
    queue: String,
    action: String,
}

#[derive(Debug, Deserialize)]
struct BundlePhaseSection {
    produce: String,
    gate: String,
}

#[derive(Debug, Deserialize)]
struct BundleProfileSection {
    #[serde(default)]
    description: Option<String>,
    phases: Vec<String>,
    #[serde(default)]
    output: Option<String>,
    #[serde(default)]
    overrides: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct BundlePromptSection {
    #[serde(default)]
    accept: Vec<String>,
    #[serde(default)]
    success: BTreeMap<String, String>,
    #[serde(default)]
    failure: BTreeMap<String, String>,
    #[serde(default)]
    body: String,
    #[serde(default)]
    params: BTreeMap<String, BundlePromptParamSection>,
}

#[derive(Debug, Deserialize)]
struct BundlePromptParamSection {
    #[serde(rename = "type")]
    param_type: String,
    #[serde(default)]
    values: Vec<String>,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    default: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Clone, Copy)]
enum BundleFormat {
    Json,
    Toml,
}

fn parse_bundle(raw: &str, format: BundleFormat) -> Result<WorkflowDefinition, ProfileError> {
    match format {
        BundleFormat::Json => parse_bundle_json(raw),
        BundleFormat::Toml => parse_bundle_toml(raw),
    }
}

fn render_json_bundle_from_toml(raw: &str) -> Result<String, ProfileError> {
    let parsed: BundleToml =
        toml::from_str(raw).map_err(|err| ProfileError::InvalidBundle(err.to_string()))?;
    let bundle = JsonKnotsBundle {
        format: "knots-bundle".to_string(),
        format_version: 1,
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
        profiles: parsed
            .profiles
            .into_iter()
            .map(|(id, profile)| JsonProfileSection {
                id,
                description: profile.description,
                display_name: None,
                phases: profile.phases,
                output: profile.output,
                executors: profile.overrides,
            })
            .collect(),
        prompts: parsed
            .prompts
            .into_iter()
            .map(|(name, prompt)| {
                let mut outcomes = prompt
                    .success
                    .into_values()
                    .map(|target| JsonPromptOutcome {
                        target,
                        is_success: true,
                    })
                    .collect::<Vec<_>>();
                outcomes.extend(prompt.failure.into_values().map(|target| JsonPromptOutcome {
                    target,
                    is_success: false,
                }));
                JsonPromptSection {
                    name,
                    accept: prompt.accept,
                    body: prompt.body,
                    outcomes,
                }
            })
            .collect(),
    };
    serde_json::to_string_pretty(&bundle)
        .map_err(|err| ProfileError::InvalidBundle(err.to_string()))
}

fn parse_bundle_toml(raw: &str) -> Result<WorkflowDefinition, ProfileError> {
    let parsed: BundleToml =
        toml::from_str(raw).map_err(|err| ProfileError::InvalidBundle(err.to_string()))?;
    let workflow_id = normalize_profile_id(&parsed.workflow.name).ok_or_else(|| {
        ProfileError::InvalidBundle("workflow.name is required".to_string())
    })?;

    let mut prompts = BTreeMap::new();
    for (prompt_name, prompt) in &parsed.prompts {
        let success_target = match prompt.success.len() {
            0 => None,
            1 => Some(prompt.success.values().next().cloned().unwrap_or_default()),
            _ => {
                return Err(ProfileError::InvalidBundle(format!(
                    "prompt '{}' has multiple success outcomes; Knots requires one happy-path target",
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
        prompts.insert(
            prompt_name.clone(),
            PromptDefinition {
                prompt_name: prompt_name.clone(),
                action_state: String::new(),
                accept: prompt.accept.clone(),
                success_target,
                failure_targets: prompt
                    .failure
                    .iter()
                    .map(|(outcome, target)| (outcome.clone(), target.clone()))
                    .collect(),
                params,
                body: prompt.body.clone(),
            },
        );
    }

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
        for action_state in &profile.action_states {
            let state = parsed.states.get(action_state).ok_or_else(|| {
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

#[derive(Debug, Serialize, Deserialize)]
struct JsonKnotsBundle {
    format: String,
    format_version: u32,
    workflow: JsonWorkflowSection,
    states: Vec<JsonStateSection>,
    steps: Vec<JsonStepSection>,
    phases: Vec<JsonPhaseSection>,
    profiles: Vec<JsonProfileSection>,
    prompts: Vec<JsonPromptSection>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonWorkflowSection {
    name: String,
    version: u32,
    default_profile: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonStateSection {
    id: String,
    kind: String,
    prompt: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonPhaseSection {
    id: String,
    produce_step: String,
    gate_step: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonStepSection {
    id: String,
    queue: String,
    action: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonProfileSection {
    id: String,
    description: Option<String>,
    display_name: Option<String>,
    phases: Vec<String>,
    output: Option<String>,
    #[serde(default)]
    executors: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonPromptSection {
    name: String,
    #[serde(default)]
    accept: Vec<String>,
    #[serde(default)]
    body: String,
    #[serde(default)]
    outcomes: Vec<JsonPromptOutcome>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonPromptOutcome {
    target: String,
    is_success: bool,
}

fn parse_bundle_json(raw: &str) -> Result<WorkflowDefinition, ProfileError> {
    let parsed: JsonKnotsBundle =
        serde_json::from_str(raw).map_err(|err| ProfileError::InvalidBundle(err.to_string()))?;
    if parsed.format != "knots-bundle" {
        return Err(ProfileError::InvalidBundle(format!(
            "unsupported bundle format '{}'",
            parsed.format
        )));
    }
    if parsed.format_version != 1 {
        return Err(ProfileError::InvalidBundle(format!(
            "unsupported bundle format version '{}'",
            parsed.format_version
        )));
    }

    let workflow_id = normalize_profile_id(&parsed.workflow.name)
        .ok_or_else(|| ProfileError::InvalidBundle("workflow.name is required".to_string()))?;
    let states_by_id = parsed
        .states
        .iter()
        .map(|state| (state.id.as_str(), state))
        .collect::<BTreeMap<_, _>>();
    let steps_by_id = parsed
        .steps
        .iter()
        .map(|step| (step.id.as_str(), step))
        .collect::<BTreeMap<_, _>>();
    let phases_by_id = parsed
        .phases
        .iter()
        .map(|phase| (phase.id.as_str(), phase))
        .collect::<BTreeMap<_, _>>();
    let prompts_by_name = parsed
        .prompts
        .iter()
        .map(|prompt| (prompt.name.as_str(), prompt))
        .collect::<BTreeMap<_, _>>();

    let mut profiles = BTreeMap::new();
    let mut action_prompts = BTreeMap::new();
    for profile in &parsed.profiles {
        let profile_id = normalize_profile_id(&profile.id)
            .ok_or_else(|| ProfileError::InvalidBundle("profile id is required".to_string()))?;
        let mut ordered_states = Vec::new();
        let mut queue_states = Vec::new();
        let mut action_states = Vec::new();
        let mut transitions = Vec::new();
        let mut owner_states = BTreeMap::new();
        let mut prompt_bodies = BTreeMap::new();
        let mut prompt_acceptance = BTreeMap::new();
        let mut first_queue = None;

        for phase_name in &profile.phases {
            let phase = phases_by_id.get(phase_name.as_str()).ok_or_else(|| {
                ProfileError::InvalidBundle(format!("unknown phase '{}'", phase_name))
            })?;
            for step_name in [&phase.produce_step, &phase.gate_step] {
                let step = steps_by_id.get(step_name.as_str()).ok_or_else(|| {
                    ProfileError::InvalidBundle(format!("unknown step '{}'", step_name))
                })?;
                let queue_name = step.queue.as_str();
                let action_name = step.action.as_str();
                push_unique(&mut ordered_states, queue_name.to_string());
                push_unique(&mut ordered_states, action_name.to_string());
                push_unique(&mut queue_states, queue_name.to_string());
                push_unique(&mut action_states, action_name.to_string());
                transitions.push(WorkflowTransition {
                    from: queue_name.to_string(),
                    to: action_name.to_string(),
                });
                let owner = default_owner(match profile
                    .executors
                    .get(action_name)
                    .map(|value| value.as_str())
                    .unwrap_or("agent")
                    .trim()
                    .to_ascii_lowercase()
                    .as_str()
                {
                    "human" => OwnerKind::Human,
                    _ => OwnerKind::Agent,
                });
                owner_states.insert(queue_name.to_string(), owner.clone());
                owner_states.insert(action_name.to_string(), owner);
                first_queue.get_or_insert_with(|| queue_name.to_string());

                if let Some(state) = states_by_id.get(action_name) {
                    if let Some(prompt_name) = state.prompt.as_deref() {
                        action_prompts.insert(action_name.to_string(), prompt_name.to_string());
                        if let Some(prompt) = prompts_by_name.get(prompt_name) {
                            prompt_bodies.insert(action_name.to_string(), prompt.body.clone());
                            prompt_acceptance
                                .insert(action_name.to_string(), prompt.accept.clone());
                            if let Some(success) = prompt.outcomes.iter().find(|outcome| outcome.is_success) {
                                transitions.push(WorkflowTransition {
                                    from: action_name.to_string(),
                                    to: success.target.clone(),
                                });
                                push_unique(&mut ordered_states, success.target.clone());
                            }
                            for failure in prompt.outcomes.iter().filter(|outcome| !outcome.is_success)
                            {
                                transitions.push(WorkflowTransition {
                                    from: action_name.to_string(),
                                    to: failure.target.clone(),
                                });
                                push_unique(&mut ordered_states, failure.target.clone());
                            }
                        }
                    }
                }
            }
        }

        let mut terminal_states = Vec::new();
        for state in &parsed.states {
            if matches!(state.kind.as_str(), "terminal" | "escape") {
                push_unique(&mut ordered_states, state.id.clone());
                push_unique(&mut terminal_states, state.id.clone());
            }
        }

        let owners = ProfileOwners {
            planning: default_owner(OwnerKind::Agent),
            plan_review: default_owner(OwnerKind::Human),
            implementation: default_owner(OwnerKind::Agent),
            implementation_review: default_owner(OwnerKind::Human),
            shipment: default_owner(OwnerKind::Agent),
            shipment_review: default_owner(OwnerKind::Human),
            states: owner_states,
        };

        profiles.insert(
            profile_id.clone(),
            ProfileDefinition {
                id: profile_id,
                workflow_id: workflow_id.clone(),
                aliases: Vec::new(),
                description: profile.description.clone().or_else(|| profile.display_name.clone()),
                planning_mode: GateMode::Required,
                implementation_review_mode: GateMode::Required,
                output: parse_output_mode(profile.output.as_deref())?,
                owners,
                initial_state: first_queue.ok_or_else(|| {
                    ProfileError::InvalidBundle("profile has no initial queue state".to_string())
                })?,
                states: ordered_states,
                queue_states,
                action_states,
                terminal_states,
                transitions,
                action_prompts: prompt_bodies,
                prompt_acceptance,
            },
        );
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
        prompts: BTreeMap::new(),
        action_prompts,
    })
}

fn build_profile_definition(
    workflow_id: &str,
    profile_name: &str,
    profile_section: &BundleProfileSection,
    states: &BTreeMap<String, BundleStateSection>,
    steps: &BTreeMap<String, BundleStepSection>,
    phases: &BTreeMap<String, BundlePhaseSection>,
    prompts: &BTreeMap<String, BundlePromptSection>,
) -> Result<ProfileDefinition, ProfileError> {
    let id = normalize_profile_id(profile_name)
        .ok_or_else(|| ProfileError::InvalidBundle("profile id is required".to_string()))?;
    if profile_section.phases.is_empty() {
        return Err(ProfileError::InvalidBundle(format!(
            "profile '{}' must define at least one phase",
            profile_name
        )));
    }

    let mut ordered_states = Vec::new();
    let mut queue_states = Vec::new();
    let mut action_states = Vec::new();
    let mut transitions = Vec::new();
    let mut owner_states = BTreeMap::new();
    let mut first_queue = None;

    for phase_name in &profile_section.phases {
        let phase = phases.get(phase_name).ok_or_else(|| {
            ProfileError::InvalidBundle(format!(
                "profile '{}' references unknown phase '{}'",
                profile_name, phase_name
            ))
        })?;
        for step_name in [&phase.produce, &phase.gate] {
            let step = steps.get(step_name).ok_or_else(|| {
                ProfileError::InvalidBundle(format!(
                    "profile '{}' references unknown step '{}'",
                    profile_name, step_name
                ))
            })?;
            let queue_state = states.get(&step.queue).ok_or_else(|| {
                ProfileError::InvalidBundle(format!(
                    "step '{}' references unknown queue state '{}'",
                    step_name, step.queue
                ))
            })?;
            let action_state = states.get(&step.action).ok_or_else(|| {
                ProfileError::InvalidBundle(format!(
                    "step '{}' references unknown action state '{}'",
                    step_name, step.action
                ))
            })?;
            if queue_state.kind != "queue" {
                return Err(ProfileError::InvalidBundle(format!(
                    "state '{}' must be a queue state",
                    step.queue
                )));
            }
            if action_state.kind != "action" {
                return Err(ProfileError::InvalidBundle(format!(
                    "state '{}' must be an action state",
                    step.action
                )));
            }

            push_unique(&mut ordered_states, step.queue.clone());
            push_unique(&mut ordered_states, step.action.clone());
            push_unique(&mut queue_states, step.queue.clone());
            push_unique(&mut action_states, step.action.clone());
            transitions.push(WorkflowTransition {
                from: step.queue.clone(),
                to: step.action.clone(),
            });
            let owner = owner_for_action_state(action_state, profile_section, &step.action);
            owner_states.insert(step.queue.clone(), owner.clone());
            owner_states.insert(step.action.clone(), owner);
            if first_queue.is_none() {
                first_queue = Some(step.queue.clone());
            }
        }
    }

    let mut terminal_states = Vec::new();
    for (name, state) in states {
        if state.kind == "terminal" {
            push_unique(&mut ordered_states, name.clone());
            push_unique(&mut terminal_states, name.clone());
        } else if state.kind == "escape" {
            push_unique(&mut ordered_states, name.clone());
        }
    }

    for action_state in &action_states {
        let state = states.get(action_state).ok_or_else(|| {
            ProfileError::InvalidBundle(format!("missing action state '{}'", action_state))
        })?;
        let prompt_name = state.prompt.as_ref().ok_or_else(|| {
            ProfileError::InvalidBundle(format!("action '{}' is missing prompt", action_state))
        })?;
        let prompt = prompts.get(prompt_name).ok_or_else(|| {
            ProfileError::InvalidBundle(format!(
                "action '{}' references unknown prompt '{}'",
                action_state, prompt_name
            ))
        })?;
        let Some(success_target) = prompt.success.values().next() else {
            return Err(ProfileError::InvalidBundle(format!(
                "prompt '{}' must define one success target",
                prompt_name
            )));
        };
        transitions.push(WorkflowTransition {
            from: action_state.clone(),
            to: success_target.clone(),
        });
        push_unique(&mut ordered_states, success_target.clone());
        for target in prompt.failure.values() {
            transitions.push(WorkflowTransition {
                from: action_state.clone(),
                to: target.clone(),
            });
            push_unique(&mut ordered_states, target.clone());
        }
    }

    transitions.push(WorkflowTransition {
        from: "*".to_string(),
        to: "deferred".to_string(),
    });
    transitions.push(WorkflowTransition {
        from: "*".to_string(),
        to: "abandoned".to_string(),
    });

    let output = parse_output_mode(profile_section.output.as_deref())?;
    let owners = ProfileOwners {
        planning: default_owner(OwnerKind::Agent),
        plan_review: default_owner(OwnerKind::Human),
        implementation: default_owner(OwnerKind::Agent),
        implementation_review: default_owner(OwnerKind::Human),
        shipment: default_owner(OwnerKind::Agent),
        shipment_review: default_owner(OwnerKind::Human),
        states: owner_states,
    };
    let action_prompts = action_states
        .iter()
        .filter_map(|state| {
            let prompt_name = states
                .get(state)
                .and_then(|definition| definition.prompt.as_ref())?;
            let definition = prompts.get(prompt_name)?;
            Some((state.clone(), definition.body.clone()))
        })
        .collect();
    let prompt_acceptance = action_states
        .iter()
        .filter_map(|state| {
            let prompt = states
                .get(state)
                .and_then(|definition| definition.prompt.as_ref())?;
            let definition = prompts.get(prompt)?;
            Some((state.clone(), definition.accept.clone()))
        })
        .collect();

    Ok(ProfileDefinition {
        id,
        workflow_id: workflow_id.to_string(),
        aliases: Vec::new(),
        description: profile_section.description.clone(),
        planning_mode: GateMode::Required,
        implementation_review_mode: GateMode::Required,
        output,
        owners,
        initial_state: first_queue.ok_or_else(|| {
            ProfileError::InvalidBundle(format!(
                "profile '{}' could not determine an initial queue state",
                profile_name
            ))
        })?,
        states: ordered_states,
        queue_states,
        action_states,
        terminal_states,
        transitions,
        action_prompts,
        prompt_acceptance,
    })
}

fn owner_for_action_state(
    state: &BundleStateSection,
    profile: &BundleProfileSection,
    action_state: &str,
) -> StepOwner {
    let raw_executor = profile
        .overrides
        .get(action_state)
        .or(state.executor.as_ref())
        .map(|value| value.trim().to_ascii_lowercase());
    let kind = match raw_executor.as_deref() {
        Some("human") => OwnerKind::Human,
        _ => OwnerKind::Agent,
    };
    default_owner(kind)
}

fn default_owner(kind: OwnerKind) -> StepOwner {
    StepOwner {
        kind,
        agent_name: None,
        agent_model: None,
        agent_version: None,
    }
}

fn parse_output_mode(raw: Option<&str>) -> Result<OutputMode, ProfileError> {
    match raw.unwrap_or("local").trim().to_ascii_lowercase().as_str() {
        "local" => Ok(OutputMode::Local),
        "remote" => Ok(OutputMode::Remote),
        "pr" => Ok(OutputMode::Pr),
        "remote_main" => Ok(OutputMode::RemoteMain),
        other => Err(ProfileError::InvalidBundle(format!(
            "unsupported output mode '{}'",
            other
        ))),
    }
}

fn push_unique(items: &mut Vec<String>, value: String) {
    if !items.iter().any(|item| item == &value) {
        items.push(value);
    }
}

#[cfg(test)]
fn build_prompt_params(
    workflow: &WorkflowDefinition,
    profile: &ProfileDefinition,
    prompt: &PromptDefinition,
) -> BTreeMap<String, String> {
    let mut params = BTreeMap::new();
    params.insert("workflow_id".to_string(), workflow.id.clone());
    params.insert("profile_id".to_string(), profile.id.clone());
    params.insert("output_kind".to_string(), output_mode_slug(&profile.output).to_string());
    for param in &prompt.params {
        if let Some(default) = param.default.as_deref() {
            params.entry(param.name.clone()).or_insert_with(|| default.to_string());
        }
    }
    params
}

#[cfg(test)]
fn render_prompt_template(
    template: &str,
    params: &BTreeMap<String, String>,
    unresolved: &mut Vec<String>,
) -> String {
    let mut rendered = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        rendered.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find("}}") else {
            rendered.push_str(&rest[start..]);
            return rendered;
        };
        let token = &after_start[..end];
        let key = token.trim();
        if let Some(value) = params.get(key) {
            rendered.push_str(value);
        } else {
            unresolved.push(key.to_string());
            rendered.push_str("{{");
            rendered.push_str(token);
            rendered.push_str("}}");
        }
        rest = &after_start[end + 2..];
    }
    rendered.push_str(rest);
    unresolved.sort();
    unresolved.dedup();
    rendered
}

#[cfg(test)]
fn output_mode_slug(mode: &OutputMode) -> &'static str {
    match mode {
        OutputMode::Local => "local",
        OutputMode::Remote => "remote",
        OutputMode::Pr => "pr",
        OutputMode::RemoteMain => "remote_main",
    }
}

impl fmt::Display for WorkflowDefinition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} v{}", self.id, self.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_BUNDLE: &str = r#"
[workflow]
name = "custom_flow"
version = 3
default_profile = "autopilot"

[states.ready_for_work]
display_name = "Ready for Work"
kind = "queue"

[states.work]
display_name = "Work"
kind = "action"
action_type = "produce"
executor = "agent"
prompt = "work"

[states.review]
display_name = "Review"
kind = "action"
action_type = "gate"
executor = "human"
prompt = "review"

[states.ready_for_review]
display_name = "Ready for Review"
kind = "queue"

[states.done]
display_name = "Done"
kind = "terminal"

[states.deferred]
display_name = "Deferred"
kind = "escape"

[states.abandoned]
display_name = "Abandoned"
kind = "terminal"

[steps.impl]
queue = "ready_for_work"
action = "work"

[steps.rev]
queue = "ready_for_review"
action = "review"

[phases.main]
produce = "impl"
gate = "rev"

[profiles.autopilot]
description = "Custom profile"
phases = ["main"]
output = "remote_main"

[prompts.work]
accept = ["Built output"]
body = """
Ship {{ output_kind }} output.
"""

[prompts.work.success]
complete = "ready_for_review"

[prompts.work.failure]
blocked = "deferred"

[prompts.review]
accept = ["Reviewed output"]
body = """
Review it.
"""

[prompts.review.success]
approved = "done"

[prompts.review.failure]
changes = "ready_for_work"
"#;

    #[test]
    fn parses_bundle_and_renders_prompt() {
        let workflow = parse_bundle_toml(SAMPLE_BUNDLE).expect("bundle should parse");
        assert_eq!(workflow.id, "custom_flow");
        let profile = workflow
            .require_profile("autopilot")
            .expect("profile should exist");
        assert_eq!(profile.initial_state, "ready_for_work");
        let prompt = workflow
            .prompt_for_action_state("work")
            .expect("prompt should exist");
        let rendered = prompt.render(&workflow, profile);
        assert!(rendered.contains("Ship remote_main output."));
        assert!(rendered.contains("Built output"));
    }
}
