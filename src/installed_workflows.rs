use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::profile::{
    normalize_profile_id, ActionOutputDef, GateMode, OwnerKind, ProfileDefinition, ProfileError,
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
                    let Some(version_name) =
                        version_path.file_name().and_then(|name| name.to_str())
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
        self.current
            .as_ref()
            .and_then(|config| config.current_version)
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
    let raw =
        fs::read_to_string(&path).map_err(|err| ProfileError::InvalidBundle(err.to_string()))?;
    toml::from_str(&raw).map_err(|err| ProfileError::InvalidBundle(err.to_string()))
}

pub fn write_repo_config(
    repo_root: &Path,
    config: &WorkflowRepoConfig,
) -> Result<(), ProfileError> {
    let path = repo_config_path(repo_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| ProfileError::InvalidBundle(err.to_string()))?;
    }
    let rendered = toml::to_string_pretty(config)
        .map_err(|err| ProfileError::InvalidBundle(err.to_string()))?;
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
            .or_else(|| {
                workflow
                    .list_profiles()
                    .into_iter()
                    .next()
                    .map(|profile| profile.id.clone())
            })
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
        let loom_bin = std::env::var("KNOTS_LOOM_BIN").unwrap_or_else(|_| "loom".to_string());
        let output = Command::new(loom_bin)
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
        let raw = String::from_utf8(output.stdout).map_err(|err| {
            ProfileError::InvalidBundle(format!("invalid UTF-8 bundle output: {err}"))
        })?;
        return Ok((raw, BundleFormat::Json));
    }

    let source_path = resolve_bundle_source_path(source)?;
    let raw = fs::read_to_string(&source_path)
        .map_err(|err| ProfileError::InvalidBundle(err.to_string()))?;
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
                .filter(|state| {
                    !profile
                        .terminal_states
                        .iter()
                        .any(|terminal| terminal == *state)
                })
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
    #[serde(default)]
    output: Option<String>,
    #[serde(default)]
    output_hint: Option<String>,
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
    outputs: BTreeMap<String, BundleOutputEntry>,
    #[serde(default)]
    overrides: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct BundleOutputEntry {
    artifact_type: String,
    #[serde(default)]
    access_hint: Option<String>,
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
        profiles: parsed
            .profiles
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
            .collect(),
        prompts: parsed
            .prompts
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
                outcomes.extend(prompt.failure.into_iter().map(|(name, target)| {
                    JsonPromptOutcome {
                        name,
                        target,
                        is_success: false,
                    }
                }));
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
            .collect(),
    };
    serde_json::to_string_pretty(&bundle)
        .map_err(|err| ProfileError::InvalidBundle(err.to_string()))
}

fn parse_bundle_toml(raw: &str) -> Result<WorkflowDefinition, ProfileError> {
    let parsed: BundleToml =
        toml::from_str(raw).map_err(|err| ProfileError::InvalidBundle(err.to_string()))?;
    let workflow_id = normalize_profile_id(&parsed.workflow.name)
        .ok_or_else(|| ProfileError::InvalidBundle("workflow.name is required".to_string()))?;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output_hint: Option<String>,
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
    #[serde(default)]
    outputs: BTreeMap<String, JsonOutputEntry>,
    #[serde(default)]
    executors: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonOutputEntry {
    artifact_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    access_hint: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonPromptSection {
    name: String,
    #[serde(default)]
    accept: Vec<String>,
    #[serde(default)]
    body: String,
    #[serde(default)]
    params: Vec<JsonPromptParamSection>,
    #[serde(default)]
    outcomes: Vec<JsonPromptOutcome>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonPromptOutcome {
    #[serde(default)]
    name: String,
    target: String,
    is_success: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonPromptParamSection {
    name: String,
    #[serde(alias = "type", alias = "param_type", rename = "type")]
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

fn parse_bundle_json(raw: &str) -> Result<WorkflowDefinition, ProfileError> {
    let parsed: JsonKnotsBundle =
        serde_json::from_str(raw).map_err(|err| ProfileError::InvalidBundle(err.to_string()))?;
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
    let mut prompts = parsed
        .prompts
        .iter()
        .map(|prompt| {
            let success_target = prompt
                .outcomes
                .iter()
                .find(|outcome| outcome.is_success)
                .map(|outcome| outcome.target.clone());
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
                        .filter(|outcome| !outcome.is_success)
                        .map(|outcome| {
                            (
                                if outcome.name.trim().is_empty() {
                                    outcome.target.clone()
                                } else {
                                    outcome.name.clone()
                                },
                                outcome.target.clone(),
                            )
                        })
                        .collect(),
                    params: prompt
                        .params
                        .iter()
                        .map(|param| PromptParamDefinition {
                            name: param.name.clone(),
                            param_type: param.param_type.clone(),
                            values: param.values.clone(),
                            required: param.required,
                            default: param.default.clone(),
                            description: param.description.clone(),
                        })
                        .collect(),
                    body: prompt.body.clone(),
                },
            )
        })
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
                let owner = default_owner(
                    match profile
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
                    },
                );
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
                            if let Some(success) =
                                prompt.outcomes.iter().find(|outcome| outcome.is_success)
                            {
                                transitions.push(WorkflowTransition {
                                    from: action_name.to_string(),
                                    to: success.target.clone(),
                                });
                                push_unique(&mut ordered_states, success.target.clone());
                            }
                            for failure in
                                prompt.outcomes.iter().filter(|outcome| !outcome.is_success)
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
                description: profile
                    .description
                    .clone()
                    .or_else(|| profile.display_name.clone()),
                planning_mode: GateMode::Required,
                implementation_review_mode: GateMode::Required,
                outputs: build_outputs_from_json_profile(
                    &profile.outputs,
                    &action_states,
                    &states_by_id,
                ),
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

    let outputs = build_outputs_from_toml_profile(&profile_section.outputs, &action_states, states);
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
        outputs,
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

fn build_outputs_from_toml_profile(
    profile_outputs: &BTreeMap<String, BundleOutputEntry>,
    action_states: &[String],
    states: &BTreeMap<String, BundleStateSection>,
) -> BTreeMap<String, ActionOutputDef> {
    let mut outputs = BTreeMap::new();
    for action in action_states {
        let def = if let Some(entry) = profile_outputs.get(action) {
            ActionOutputDef {
                artifact_type: entry.artifact_type.clone(),
                access_hint: entry.access_hint.clone(),
            }
        } else if let Some(state) = states.get(action) {
            ActionOutputDef {
                artifact_type: state.output.clone().unwrap_or_default(),
                access_hint: state.output_hint.clone(),
            }
        } else {
            continue;
        };
        outputs.insert(action.clone(), def);
    }
    outputs
}

fn build_outputs_from_json_profile(
    profile_outputs: &BTreeMap<String, JsonOutputEntry>,
    action_states: &[String],
    states_by_id: &BTreeMap<&str, &JsonStateSection>,
) -> BTreeMap<String, ActionOutputDef> {
    let mut outputs = BTreeMap::new();
    for action in action_states {
        let def = if let Some(entry) = profile_outputs.get(action) {
            ActionOutputDef {
                artifact_type: entry.artifact_type.clone(),
                access_hint: entry.access_hint.clone(),
            }
        } else if let Some(state) = states_by_id.get(action.as_str()) {
            ActionOutputDef {
                artifact_type: state.output.clone().unwrap_or_default(),
                access_hint: state.output_hint.clone(),
            }
        } else {
            continue;
        };
        outputs.insert(action.clone(), def);
    }
    outputs
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
    if let Some(output_def) = profile.outputs.get(&prompt.action_state) {
        params.insert("output".to_string(), output_def.artifact_type.clone());
        if let Some(hint) = &output_def.access_hint {
            params.insert("output_hint".to_string(), hint.clone());
        }
    }
    for param in &prompt.params {
        if let Some(default) = param.default.as_deref() {
            params
                .entry(param.name.clone())
                .or_insert_with(|| default.to_string());
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

impl fmt::Display for WorkflowDefinition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} v{}", self.id, self.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};
    use uuid::Uuid;

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
output = "branch"
output_hint = "git log"

[states.review]
display_name = "Review"
kind = "action"
action_type = "gate"
executor = "human"
prompt = "review"
output = "note"

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

[prompts.work]
accept = ["Built output"]
body = """
Ship {{ output }} output.
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

    fn unique_workspace(prefix: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&root).expect("temp workspace should be creatable");
        root
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

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
        assert!(rendered.contains("Ship branch output."));
        assert!(rendered.contains("Built output"));
    }

    #[test]
    fn repo_config_round_trips_through_disk() {
        let root = unique_workspace("knots-installed-workflows-config");
        let config = WorkflowRepoConfig {
            current_workflow: Some("custom_flow".to_string()),
            current_version: Some(3),
            current_profile: Some("custom_flow/autopilot".to_string()),
        };
        write_repo_config(&root, &config).expect("config should write");
        let loaded = read_repo_config(&root).expect("config should load");
        assert_eq!(loaded, config);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn install_bundle_writes_repo_local_registry_and_sets_current_selection() {
        let root = unique_workspace("knots-installed-workflows-install");
        let source = root.join("custom-flow.toml");
        std::fs::write(&source, SAMPLE_BUNDLE).expect("bundle should write");

        let workflow_id = install_bundle(&root, &source).expect("bundle should install");
        assert_eq!(workflow_id, "custom_flow");

        let version_dir = workflows_root(&root).join("custom_flow/3");
        assert!(version_dir.join("bundle.json").exists());
        assert!(version_dir.join("bundle.toml").exists());
        assert!(workflows_root(&root)
            .join("custom_flow/bundle.json")
            .exists());

        let current = read_repo_config(&root).expect("current config should load");
        assert_eq!(current.current_workflow.as_deref(), Some("custom_flow"));
        assert_eq!(current.current_version, Some(3));
        assert_eq!(
            current.current_profile.as_deref(),
            Some("custom_flow/autopilot")
        );

        let registry = InstalledWorkflowRegistry::load(&root).expect("registry should load");
        let workflow = registry
            .current_workflow()
            .expect("current workflow should resolve");
        assert_eq!(workflow.id, "custom_flow");
        assert_eq!(registry.current_profile_id(), Some("custom_flow/autopilot"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn set_current_workflow_selection_keeps_builtin_profiles_unscoped() {
        let root = unique_workspace("knots-installed-workflows-builtin");
        let config =
            set_current_workflow_selection(&root, COMPATIBILITY_WORKFLOW_ID, Some(1), None)
                .expect("builtin workflow should select");
        assert_eq!(
            config.current_workflow.as_deref(),
            Some(COMPATIBILITY_WORKFLOW_ID)
        );
        assert_eq!(config.current_profile.as_deref(), Some("autopilot"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resolve_bundle_source_path_finds_supported_candidates_and_errors_cleanly() {
        let root = unique_workspace("knots-installed-workflows-resolve");
        let candidate_dir = root.join("bundle-dir");
        std::fs::create_dir_all(candidate_dir.join("dist")).expect("dist should exist");
        std::fs::write(candidate_dir.join("dist/bundle.toml"), SAMPLE_BUNDLE)
            .expect("bundle should write");
        let resolved =
            resolve_bundle_source_path(&candidate_dir).expect("candidate bundle should resolve");
        assert!(resolved.ends_with("dist/bundle.toml"));

        let missing_dir = root.join("missing");
        std::fs::create_dir_all(&missing_dir).expect("missing dir should exist");
        let err = resolve_bundle_source_path(&missing_dir).expect_err("empty dir should fail");
        assert!(err.to_string().contains("no Loom bundle found"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn installed_bundle_path_prefers_json_and_falls_back_to_toml() {
        let root = unique_workspace("knots-installed-workflows-installed-path");
        let workflow_dir = root.join("custom_flow/3");
        std::fs::create_dir_all(&workflow_dir).expect("workflow dir should exist");
        std::fs::write(workflow_dir.join("bundle.toml"), SAMPLE_BUNDLE)
            .expect("toml bundle should write");
        assert_eq!(
            installed_bundle_path(&workflow_dir),
            Some(workflow_dir.join("bundle.toml"))
        );
        std::fs::write(workflow_dir.join("bundle.json"), "{}").expect("json bundle should write");
        assert_eq!(
            installed_bundle_path(&workflow_dir),
            Some(workflow_dir.join("bundle.json"))
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn json_bundle_round_trips_and_preserves_prompt_routes() {
        let rendered =
            render_json_bundle_from_toml(SAMPLE_BUNDLE).expect("json render should work");
        let workflow = parse_bundle_json(&rendered).expect("json bundle should parse");
        let profile = workflow
            .require_profile("autopilot")
            .expect("profile should exist");
        assert_eq!(profile.initial_state, "ready_for_work");
        assert_eq!(
            profile.next_happy_path_state("work"),
            Some("ready_for_review")
        );
        assert_eq!(
            profile.prompt_for_action_state("review"),
            Some("Review it.\n")
        );
        assert_eq!(
            profile.acceptance_for_action_state("work"),
            &["Built output".to_string()]
        );
    }

    #[test]
    fn parse_bundle_json_rejects_unsupported_metadata() {
        let wrong_format = r#"{
  "format": "other",
  "format_version": 1,
  "workflow": {"name": "x", "version": 1, "default_profile": null},
  "states": [],
  "steps": [],
  "phases": [],
  "profiles": [],
  "prompts": []
}"#;
        let err = parse_bundle_json(wrong_format).expect_err("unknown format should fail");
        assert!(err.to_string().contains("unsupported bundle format"));

        let wrong_version = r#"{
  "format": "knots-bundle",
  "format_version": 99,
  "workflow": {"name": "x", "version": 1, "default_profile": null},
  "states": [],
  "steps": [],
  "phases": [],
  "profiles": [],
  "prompts": []
}"#;
        let err = parse_bundle_json(wrong_version).expect_err("unknown version should fail");
        assert!(err
            .to_string()
            .contains("unsupported bundle format version"));
    }

    #[test]
    fn parse_bundle_toml_rejects_multiple_success_outcomes() {
        let invalid = SAMPLE_BUNDLE.replace(
            "[prompts.work.success]\ncomplete = \"ready_for_review\"\n",
            "[prompts.work.success]\ncomplete = \"ready_for_review\"\nalso_complete = \"done\"\n",
        );
        let err = parse_bundle_toml(&invalid).expect_err("multiple success routes should fail");
        assert!(err.to_string().contains("multiple success outcomes"));
    }

    #[test]
    fn parse_bundle_toml_reads_per_action_outputs() {
        let workflow = parse_bundle_toml(SAMPLE_BUNDLE).expect("bundle should parse");
        let profile = workflow
            .require_profile("autopilot")
            .expect("profile should exist");
        let work_output = profile
            .outputs
            .get("work")
            .expect("work output should exist");
        assert_eq!(work_output.artifact_type, "branch");
        assert_eq!(work_output.access_hint.as_deref(), Some("git log"));
        let review_output = profile
            .outputs
            .get("review")
            .expect("review output should exist");
        assert_eq!(review_output.artifact_type, "note");
        assert!(review_output.access_hint.is_none());
    }

    #[test]
    fn compatibility_workflow_exposes_builtin_prompts_and_profiles() {
        let workflow = compatibility_workflow().expect("compatibility workflow should build");
        assert!(workflow.builtin);
        assert_eq!(workflow.default_profile.as_deref(), Some("autopilot"));
        assert!(workflow.prompts.contains_key("planning"));
        assert!(workflow.action_prompts.contains_key("implementation"));
        assert!(workflow.require_profile("autopilot").is_ok());
    }

    #[test]
    fn workflow_registry_helpers_cover_lookup_and_sorting_paths() {
        let root = unique_workspace("knots-installed-workflows-registry");
        assert_eq!(
            InstalledWorkflowRegistry::load(&root)
                .expect("registry should load")
                .current_workflow_id(),
            COMPATIBILITY_WORKFLOW_ID
        );

        let source = root.join("custom-flow.toml");
        std::fs::write(&source, SAMPLE_BUNDLE).expect("bundle should write");
        install_bundle(&root, &source).expect("bundle should install");

        let registry = InstalledWorkflowRegistry::load(&root).expect("registry should load");
        assert_eq!(registry.current_workflow_version(), Some(3));
        assert_eq!(registry.current_profile_id(), Some("custom_flow/autopilot"));
        assert_eq!(
            registry
                .require_workflow("custom_flow")
                .expect("workflow should exist")
                .to_string(),
            "custom_flow v3"
        );
        assert!(registry.require_workflow("missing").is_err());
        assert!(registry
            .require_workflow_version("custom_flow", 99)
            .is_err());

        let listed = registry
            .list()
            .into_iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        assert_eq!(listed, vec!["compatibility v1", "custom_flow v3"]);

        let workflow = registry
            .require_workflow_version("custom_flow", 3)
            .expect("workflow should exist");
        assert_eq!(workflow.display_description(), None);
        assert_eq!(workflow.list_profiles().len(), 1);
        assert!(workflow.require_profile("missing").is_err());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn read_bundle_source_supports_file_and_directory_inputs() {
        let root = unique_workspace("knots-installed-workflows-read-source");
        let toml_path = root.join("bundle.toml");
        std::fs::write(&toml_path, SAMPLE_BUNDLE).expect("bundle should write");
        let (raw, format) = read_bundle_source(&toml_path).expect("toml bundle should load");
        assert_eq!(raw, SAMPLE_BUNDLE);
        assert!(matches!(format, BundleFormat::Toml));

        let json_dir = root.join("json");
        std::fs::create_dir_all(&json_dir).expect("dir should exist");
        let json_bundle = render_json_bundle_from_toml(SAMPLE_BUNDLE).expect("json render");
        std::fs::write(json_dir.join("bundle.json"), &json_bundle).expect("json bundle writes");
        let (raw, format) = read_bundle_source(&json_dir).expect("json dir should load");
        assert_eq!(raw, json_bundle);
        assert!(matches!(format, BundleFormat::Json));

        let err = read_bundle_source(&root.join("does-not-exist")).expect_err("missing source");
        assert!(err.to_string().contains("does not exist"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn parse_bundle_dispatches_for_both_supported_formats() {
        let json_bundle = render_json_bundle_from_toml(SAMPLE_BUNDLE).expect("json render");
        let from_toml = parse_bundle(SAMPLE_BUNDLE, BundleFormat::Toml).expect("toml parse");
        let from_json = parse_bundle(&json_bundle, BundleFormat::Json).expect("json parse");
        assert_eq!(from_toml.id, from_json.id);
        assert_eq!(from_toml.version, from_json.version);
    }

    #[test]
    fn parse_bundle_toml_reports_missing_phase_and_step_references() {
        let missing_phase = SAMPLE_BUNDLE.replace("phases = [\"main\"]", "phases = [\"missing\"]");
        let err = parse_bundle_toml(&missing_phase).expect_err("missing phase should fail");
        assert!(err.to_string().contains("unknown phase"));

        let missing_step = SAMPLE_BUNDLE.replace("produce = \"impl\"", "produce = \"missing\"");
        let err = parse_bundle_toml(&missing_step).expect_err("missing step should fail");
        assert!(err.to_string().contains("unknown step"));
    }

    #[test]
    fn parse_bundle_toml_reports_invalid_state_kinds_and_prompt_metadata() {
        let invalid_queue = SAMPLE_BUNDLE.replace(
            "[states.ready_for_work]\ndisplay_name = \"Ready for Work\"\nkind = \"queue\"\n",
            "[states.ready_for_work]\ndisplay_name = \"Ready for Work\"\nkind = \"action\"\n",
        );
        let err = parse_bundle_toml(&invalid_queue).expect_err("queue kind should fail");
        assert!(err.to_string().contains("must be a queue state"));

        let invalid_action = SAMPLE_BUNDLE.replace(
            "[states.work]\ndisplay_name = \"Work\"\nkind = \"action\"\naction_type = \"produce\"\nexecutor = \"agent\"\nprompt = \"work\"\n",
            "[states.work]\ndisplay_name = \"Work\"\nkind = \"queue\"\naction_type = \"produce\"\nexecutor = \"agent\"\nprompt = \"work\"\n",
        );
        let err = parse_bundle_toml(&invalid_action).expect_err("action kind should fail");
        assert!(err.to_string().contains("must be an action state"));

        let missing_prompt = SAMPLE_BUNDLE.replace("prompt = \"work\"\n", "");
        let err = parse_bundle_toml(&missing_prompt).expect_err("missing prompt should fail");
        assert!(err.to_string().contains("missing prompt"));

        let unknown_prompt = SAMPLE_BUNDLE.replace("prompt = \"work\"", "prompt = \"missing\"");
        let err = parse_bundle_toml(&unknown_prompt).expect_err("unknown prompt should fail");
        assert!(err.to_string().contains("references unknown prompt"));
    }

    #[test]
    fn parse_bundle_toml_requires_success_targets_and_honors_owner_overrides() {
        let missing_success = SAMPLE_BUNDLE.replace(
            "[prompts.work.success]\ncomplete = \"ready_for_review\"\n",
            "",
        );
        let err = parse_bundle_toml(&missing_success).expect_err("missing success should fail");
        assert!(err.to_string().contains("must define one success target"));

        let overridden = SAMPLE_BUNDLE.replace(
            "[profiles.autopilot]\ndescription = \"Custom profile\"\nphases = [\"main\"]\n",
            "[profiles.autopilot]\ndescription = \"Custom profile\"\nphases = [\"main\"]\noverrides.work = \"human\"\n",
        );
        let workflow = parse_bundle_toml(&overridden).expect("override bundle should parse");
        let profile = workflow
            .require_profile("autopilot")
            .expect("profile should exist");
        assert_eq!(
            profile.owners.owner_kind_for_state("work"),
            Some(&OwnerKind::Human)
        );
        assert_eq!(
            profile.owners.owner_kind_for_state("ready_for_work"),
            Some(&OwnerKind::Human)
        );
    }

    #[test]
    fn helper_functions_cover_prompt_rendering_and_utilities() {
        assert_eq!(
            namespaced_profile_id("custom", "autopilot"),
            "custom/autopilot"
        );

        let mut values = vec!["a".to_string()];
        push_unique(&mut values, "a".to_string());
        push_unique(&mut values, "b".to_string());
        assert_eq!(values, vec!["a".to_string(), "b".to_string()]);

        let mut unresolved = Vec::new();
        let rendered = render_prompt_template(
            "Hello {{ name }} and {{ missing }} {{ missing }}",
            &BTreeMap::from([(String::from("name"), String::from("Loom"))]),
            &mut unresolved,
        );
        assert_eq!(rendered, "Hello Loom and {{ missing }} {{ missing }}");
        assert_eq!(unresolved, vec!["missing".to_string()]);

        let mut unresolved = Vec::new();
        let rendered = render_prompt_template("{{ name ", &BTreeMap::new(), &mut unresolved);
        assert_eq!(rendered, "{{ name ");
        assert!(unresolved.is_empty());
    }

    #[test]
    fn read_bundle_source_can_shell_out_to_loom_for_package_directories() {
        let _guard = env_lock().lock().expect("env lock");
        let root = unique_workspace("knots-installed-workflows-loom-dir");
        let bin_dir = root.join("bin");
        let package_dir = root.join("pkg");
        std::fs::create_dir_all(&bin_dir).expect("bin dir should exist");
        std::fs::create_dir_all(&package_dir).expect("package dir should exist");
        std::fs::write(package_dir.join("loom.toml"), "name = 'pkg'").expect("loom.toml writes");

        let json_bundle = render_json_bundle_from_toml(SAMPLE_BUNDLE).expect("json render");
        let loom_script = format!(
            "#!/bin/sh\nif [ \"$1\" = \"build\" ] && [ \"$3\" = \"--emit\" ] && [ \"$4\" = \"knots-bundle\" ]; then\ncat <<'EOF'\n{json_bundle}\nEOF\nelse\nexit 1\nfi\n"
        );
        let loom_path = bin_dir.join("loom");
        std::fs::write(&loom_path, loom_script).expect("loom script writes");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&loom_path)
                .expect("metadata")
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&loom_path, perms).expect("permissions");
        }

        let original_path = std::env::var_os("PATH");
        let joined_path = match &original_path {
            Some(path) => {
                let mut paths = vec![bin_dir.clone()];
                paths.extend(std::env::split_paths(path));
                std::env::join_paths(paths).expect("joined path")
            }
            None => std::env::join_paths([bin_dir.clone()]).expect("joined path"),
        };
        std::env::set_var("PATH", joined_path);

        let (raw, format) = read_bundle_source(&package_dir).expect("loom package should build");
        assert!(matches!(format, BundleFormat::Json));
        assert!(raw.contains("\"format\": \"knots-bundle\""));

        match original_path {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn registry_prefers_latest_version_when_current_selection_has_no_version() {
        let root = unique_workspace("knots-installed-workflows-latest");
        let v3 = root.join("custom-v3.toml");
        let v4 = root.join("custom-v4.toml");
        std::fs::write(&v3, SAMPLE_BUNDLE).expect("v3 writes");
        std::fs::write(&v4, SAMPLE_BUNDLE.replace("version = 3", "version = 4"))
            .expect("v4 writes");
        install_bundle(&root, &v3).expect("v3 installs");
        install_bundle(&root, &v4).expect("v4 installs");
        write_repo_config(
            &root,
            &WorkflowRepoConfig {
                current_workflow: Some("custom_flow".to_string()),
                current_version: None,
                current_profile: None,
            },
        )
        .expect("config writes");

        let registry = InstalledWorkflowRegistry::load(&root).expect("registry should load");
        let current = registry
            .current_workflow()
            .expect("current workflow resolves");
        assert_eq!(current.version, 4);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn set_current_workflow_selection_honors_explicit_profile() {
        let root = unique_workspace("knots-installed-workflows-explicit-profile");
        let source = root.join("custom-flow.toml");
        std::fs::write(&source, SAMPLE_BUNDLE).expect("bundle should write");
        install_bundle(&root, &source).expect("bundle should install");

        let config =
            set_current_workflow_selection(&root, "custom_flow", Some(3), Some("autopilot"))
                .expect("selection should succeed");
        assert_eq!(config.current_version, Some(3));
        assert_eq!(
            config.current_profile.as_deref(),
            Some("custom_flow/autopilot")
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn parse_bundle_json_validates_profile_references_and_shape() {
        let rendered = render_json_bundle_from_toml(SAMPLE_BUNDLE).expect("json render");
        let mut json: serde_json::Value =
            serde_json::from_str(&rendered).expect("json bundle should parse as value");

        json["profiles"][0]["phases"] = serde_json::json!(["missing"]);
        let err = parse_bundle_json(&serde_json::to_string(&json).expect("serialize"))
            .expect_err("unknown phase should fail");
        assert!(err.to_string().contains("unknown phase"));

        let mut json: serde_json::Value =
            serde_json::from_str(&rendered).expect("json bundle should parse as value");
        json["phases"][0]["produce_step"] = serde_json::json!("missing");
        let err = parse_bundle_json(&serde_json::to_string(&json).expect("serialize"))
            .expect_err("unknown step should fail");
        assert!(err.to_string().contains("unknown step"));

        let mut json: serde_json::Value =
            serde_json::from_str(&rendered).expect("json bundle should parse as value");
        json["profiles"][0]["phases"] = serde_json::json!([]);
        let err = parse_bundle_json(&serde_json::to_string(&json).expect("serialize"))
            .expect_err("empty phases should fail");
        assert!(err
            .to_string()
            .contains("profile has no initial queue state"));

        let mut json: serde_json::Value =
            serde_json::from_str(&rendered).expect("json bundle should parse as value");
        json["profiles"][0]["id"] = serde_json::json!("   ");
        let err = parse_bundle_json(&serde_json::to_string(&json).expect("serialize"))
            .expect_err("blank profile id should fail");
        assert!(err.to_string().contains("profile id is required"));
    }

    #[test]
    fn registry_load_skips_non_version_entries_and_accepts_legacy_toml_bundle() {
        let root = unique_workspace("knots-installed-workflows-load-legacy");
        let workflow_root = workflows_root(&root).join("legacy_flow");
        std::fs::create_dir_all(&workflow_root).expect("workflow root should exist");
        std::fs::write(workflow_root.join("README.txt"), "ignore me").expect("file should write");
        std::fs::create_dir_all(workflow_root.join("not-a-version")).expect("dir should exist");
        std::fs::create_dir_all(workflow_root.join("7")).expect("version dir should exist");
        std::fs::write(
            workflow_root.join("7/bundle.toml"),
            SAMPLE_BUNDLE.replace("custom_flow", "legacy_flow"),
        )
        .expect("legacy bundle should write");

        let registry = InstalledWorkflowRegistry::load(&root).expect("registry should load");
        let workflow = registry
            .require_workflow("legacy_flow")
            .expect("legacy flow should load from bundle.toml");
        assert_eq!(workflow.id, "legacy_flow");
        assert_eq!(workflow.version, 3);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn install_bundle_supports_json_input_without_re_rendering() {
        let root = unique_workspace("knots-installed-workflows-json-install");
        let source = root.join("bundle.json");
        let json_bundle = render_json_bundle_from_toml(SAMPLE_BUNDLE).expect("json render");
        std::fs::write(&source, &json_bundle).expect("json bundle should write");

        let workflow_id = install_bundle(&root, &source).expect("json bundle should install");
        assert_eq!(workflow_id, "custom_flow");
        let installed =
            std::fs::read_to_string(workflows_root(&root).join("custom_flow/3/bundle.json"))
                .expect("installed json should read");
        assert_eq!(installed, json_bundle);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn set_current_workflow_selection_falls_back_to_first_profile_when_default_is_missing() {
        let root = unique_workspace("knots-installed-workflows-first-profile");
        let source = root.join("bundle.toml");
        let bundle = SAMPLE_BUNDLE
            .replace("default_profile = \"autopilot\"\n", "")
            .replace(
                "[profiles.autopilot]\ndescription = \"Custom profile\"\nphases = [\"main\"]\n",
                "[profiles.beta]\ndescription = \"Beta\"\nphases = [\"main\"]\n\n[profiles.alpha]\ndescription = \"Alpha\"\nphases = [\"main\"]\n",
            );
        std::fs::write(&source, bundle).expect("bundle should write");
        install_bundle(&root, &source).expect("bundle should install");

        let config =
            set_current_workflow_selection(&root, "custom_flow", Some(3), None).expect("select");
        assert_eq!(config.current_profile.as_deref(), Some("custom_flow/alpha"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn read_bundle_source_reports_loom_failures_and_invalid_utf8() {
        let _guard = env_lock().lock().expect("env lock");
        let root = unique_workspace("knots-installed-workflows-loom-errors");
        let bin_dir = root.join("bin");
        let package_dir = root.join("pkg");
        std::fs::create_dir_all(&bin_dir).expect("bin dir should exist");
        std::fs::create_dir_all(&package_dir).expect("pkg dir should exist");
        std::fs::write(package_dir.join("loom.toml"), "name = 'pkg'").expect("loom.toml writes");

        let loom_path = bin_dir.join("loom");
        let original_path = std::env::var_os("PATH");
        let joined_path = match &original_path {
            Some(path) => {
                let mut paths = vec![bin_dir.clone()];
                paths.extend(std::env::split_paths(path));
                std::env::join_paths(paths).expect("joined path")
            }
            None => std::env::join_paths([bin_dir.clone()]).expect("joined path"),
        };
        std::env::set_var("PATH", joined_path);

        std::fs::write(&loom_path, "#!/bin/sh\necho boom >&2\nexit 1\n").expect("script writes");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&loom_path)
                .expect("metadata")
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&loom_path, perms).expect("permissions");
        }
        let err = read_bundle_source(&package_dir).expect_err("loom failure should bubble up");
        assert!(err
            .to_string()
            .contains("loom build --emit knots-bundle failed"));

        std::fs::write(
            &loom_path,
            "#!/bin/sh\nif [ \"$1\" = \"build\" ]; then\nprintf '\\377\\376'\nexit 0\nfi\nexit 1\n",
        )
        .expect("invalid utf8 script writes");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&loom_path)
                .expect("metadata")
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&loom_path, perms).expect("permissions");
        }
        let err = read_bundle_source(&package_dir).expect_err("invalid utf8 should fail");
        assert!(err.to_string().contains("invalid UTF-8 bundle output"));

        match original_path {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn build_profile_definition_validates_empty_phase_and_missing_states() {
        let mut states = BTreeMap::new();
        states.insert(
            "ready".to_string(),
            BundleStateSection {
                kind: "queue".to_string(),
                executor: None,
                prompt: None,
                output: None,
                output_hint: None,
            },
        );
        states.insert(
            "work".to_string(),
            BundleStateSection {
                kind: "action".to_string(),
                executor: None,
                prompt: Some("work".to_string()),
                output: None,
                output_hint: None,
            },
        );
        let mut steps = BTreeMap::new();
        steps.insert(
            "step".to_string(),
            BundleStepSection {
                queue: "ready".to_string(),
                action: "work".to_string(),
            },
        );
        let mut phases = BTreeMap::new();
        phases.insert(
            "phase".to_string(),
            BundlePhaseSection {
                produce: "step".to_string(),
                gate: "step".to_string(),
            },
        );
        let mut prompts = BTreeMap::new();
        prompts.insert(
            "work".to_string(),
            BundlePromptSection {
                accept: Vec::new(),
                success: BTreeMap::from([(String::from("ok"), String::from("done"))]),
                failure: BTreeMap::new(),
                body: String::from("do work"),
                params: BTreeMap::new(),
            },
        );

        let empty_profile = BundleProfileSection {
            description: None,
            phases: Vec::new(),
            outputs: BTreeMap::new(),
            overrides: BTreeMap::new(),
        };
        let err = build_profile_definition(
            "wf",
            "empty",
            &empty_profile,
            &states,
            &steps,
            &phases,
            &prompts,
        )
        .expect_err("empty profile should fail");
        assert!(err.to_string().contains("must define at least one phase"));

        steps.insert(
            "broken".to_string(),
            BundleStepSection {
                queue: "missing".to_string(),
                action: "work".to_string(),
            },
        );
        phases.insert(
            "broken".to_string(),
            BundlePhaseSection {
                produce: "broken".to_string(),
                gate: "broken".to_string(),
            },
        );
        let err = build_profile_definition(
            "wf",
            "broken",
            &BundleProfileSection {
                description: None,
                phases: vec!["broken".to_string()],
                outputs: BTreeMap::new(),
                overrides: BTreeMap::new(),
            },
            &states,
            &steps,
            &phases,
            &prompts,
        )
        .expect_err("missing queue should fail");
        assert!(err.to_string().contains("unknown queue state"));

        let err = build_profile_definition(
            "wf",
            "broken-action",
            &BundleProfileSection {
                description: None,
                phases: vec!["broken".to_string()],
                outputs: BTreeMap::new(),
                overrides: BTreeMap::new(),
            },
            &states,
            &BTreeMap::from([(
                String::from("broken"),
                BundleStepSection {
                    queue: "ready".to_string(),
                    action: "missing-action".to_string(),
                },
            )]),
            &BTreeMap::from([(
                String::from("broken"),
                BundlePhaseSection {
                    produce: "broken".to_string(),
                    gate: "broken".to_string(),
                },
            )]),
            &prompts,
        )
        .expect_err("missing action should fail");
        assert!(err.to_string().contains("unknown action state"));

        states.insert(
            "orphan".to_string(),
            BundleStateSection {
                kind: "action".to_string(),
                executor: None,
                prompt: None,
                output: None,
                output_hint: None,
            },
        );
        let err = build_profile_definition(
            "wf",
            "orphan",
            &BundleProfileSection {
                description: None,
                phases: vec!["broken".to_string()],
                outputs: BTreeMap::new(),
                overrides: BTreeMap::new(),
            },
            &states,
            &BTreeMap::from([(
                String::from("broken"),
                BundleStepSection {
                    queue: "ready".to_string(),
                    action: "orphan".to_string(),
                },
            )]),
            &BTreeMap::from([(
                String::from("broken"),
                BundlePhaseSection {
                    produce: "broken".to_string(),
                    gate: "broken".to_string(),
                },
            )]),
            &prompts,
        )
        .expect_err("missing prompt metadata should fail");
        assert!(err.to_string().contains("is missing prompt"));
    }

    #[test]
    fn prompt_defaults_cover_param_and_output_injection() {
        let workflow = parse_bundle_toml(
            &SAMPLE_BUNDLE.replace(
                "[prompts.work]\naccept = [\"Built output\"]\nbody = \"\"\"\nShip {{ output }} output.\n\"\"\"\n",
                "[prompts.work]\naccept = [\"Built output\"]\nbody = \"\"\"\nShip {{ output }} output for {{ audience }}.\n\"\"\"\n[prompts.work.params.audience]\ntype = \"enum\"\ndefault = \"operators\"\n",
            ),
        )
        .expect("bundle with params should parse");
        let profile = workflow
            .require_profile("autopilot")
            .expect("profile should exist");
        let prompt = workflow
            .prompt_for_action_state("work")
            .expect("prompt should exist");
        let params = build_prompt_params(&workflow, profile, prompt);
        assert_eq!(
            params.get("audience").map(String::as_str),
            Some("operators")
        );
        assert_eq!(params.get("output").map(String::as_str), Some("branch"));
    }
}
