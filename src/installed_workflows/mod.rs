pub(crate) mod bundle_json;
pub(crate) mod bundle_toml;
mod compatibility;
mod loader;
mod operations;
pub(crate) mod profile_json;
pub(crate) mod profile_toml;

#[cfg(test)]
mod tests_helpers;
#[cfg(test)]
mod tests_parsing;
#[cfg(test)]
mod tests_registry;
#[cfg(test)]
mod tests_registry_ext;
#[cfg(test)]
mod tests_validation;

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::profile::{normalize_profile_id, ProfileDefinition, ProfileError};

use bundle_json::parse_bundle_json;
use bundle_toml::parse_bundle_toml;

pub use operations::{
    install_bundle, namespaced_profile_id, read_repo_config, set_current_workflow_selection,
    set_workflow_default_profile,
};

pub const COMPATIBILITY_WORKFLOW_ID: &str = "compatibility";
const DEFAULT_BUNDLE_FILE: &str = "bundle.json";
const TOML_BUNDLE_FILE: &str = "bundle.toml";

// ── WorkflowRepoConfig ─────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowRepoConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_workflow: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(alias = "current_profile")]
    pub legacy_current_profile: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub default_profiles: BTreeMap<String, String>,
}

impl WorkflowRepoConfig {
    pub(crate) fn normalize(mut self) -> Self {
        if let (Some(wf_id), Some(profile_id)) = (
            self.current_workflow.as_deref(),
            self.legacy_current_profile.take(),
        ) {
            self.default_profiles
                .entry(wf_id.to_string())
                .or_insert(profile_id);
        }
        self
    }

    pub(crate) fn current_profile_id(&self) -> Option<&str> {
        self.current_workflow
            .as_deref()
            .and_then(|id| self.default_profiles.get(id).map(String::as_str))
    }

    pub(crate) fn set_default_profile(&mut self, workflow_id: &str, profile_id: String) {
        self.default_profiles
            .insert(workflow_id.to_string(), profile_id);
    }

    pub(crate) fn default_profile_id_for_workflow(&self, workflow_id: &str) -> Option<&str> {
        self.default_profiles.get(workflow_id).map(String::as_str)
    }
}

// ── Prompt types ────────────────────────────────────────

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
        let params = tests_helpers::build_prompt_params(workflow, profile, self);
        let mut unresolved = Vec::new();
        let mut body = tests_helpers::render_prompt_template(&self.body, &params, &mut unresolved);
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

// ── WorkflowDefinition ─────────────────────────────────

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
        let name = self.action_prompts.get(state)?;
        self.prompts.get(name)
    }

    pub fn display_description(&self) -> Option<&str> {
        self.description.as_deref()
    }
}

impl fmt::Display for WorkflowDefinition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} v{}", self.id, self.version)
    }
}

// ── InstalledWorkflowRegistry ───────────────────────────

#[derive(Debug, Clone)]
pub struct InstalledWorkflowRegistry {
    workflows: BTreeMap<String, BTreeMap<u32, WorkflowDefinition>>,
    current: Option<WorkflowRepoConfig>,
}

impl InstalledWorkflowRegistry {
    pub fn load(repo_root: &Path) -> Result<Self, ProfileError> {
        let mut workflows: BTreeMap<String, BTreeMap<u32, WorkflowDefinition>> = BTreeMap::new();
        let compat = compatibility::compatibility_workflow()?;
        workflows
            .entry(compat.id.clone())
            .or_default()
            .insert(compat.version, compat);
        let root = workflows_root(repo_root);
        if root.exists() {
            loader::load_disk_workflows(&root, &mut workflows)?;
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
            .and_then(|c| c.current_workflow.as_deref())
            .unwrap_or(COMPATIBILITY_WORKFLOW_ID)
    }

    pub fn current_workflow_version(&self) -> Option<u32> {
        self.current.as_ref().and_then(|c| c.current_version)
    }

    pub fn current_profile_id(&self) -> Option<String> {
        self.default_profile_id_for_workflow(self.current_workflow_id())
    }

    pub fn default_profile_id_for_workflow(&self, workflow_id: &str) -> Option<String> {
        if let Some(pid) = self
            .current
            .as_ref()
            .and_then(|c| c.default_profile_id_for_workflow(workflow_id))
        {
            return Some(pid.to_string());
        }
        let wf = self.require_workflow(workflow_id).ok()?;
        let dp = wf
            .default_profile
            .as_deref()
            .or_else(|| wf.profiles.keys().next().map(String::as_str))?;
        if wf.builtin {
            Some(dp.to_string())
        } else {
            Some(namespaced_profile_id(workflow_id, dp))
        }
    }

    pub fn current_workflow(&self) -> Result<&WorkflowDefinition, ProfileError> {
        if let Some(cfg) = self.current.as_ref() {
            if let Some(id) = cfg.current_workflow.as_deref() {
                if let Some(v) = cfg.current_version {
                    return self.require_workflow_version(id, v);
                }
                return self.require_workflow(id);
            }
        }
        self.require_workflow(COMPATIBILITY_WORKFLOW_ID)
    }

    pub fn require_workflow(&self, workflow_id: &str) -> Result<&WorkflowDefinition, ProfileError> {
        let id = normalize_profile_id(workflow_id)
            .ok_or_else(|| ProfileError::UnknownWorkflow(workflow_id.to_string()))?;
        self.workflows
            .get(&id)
            .and_then(|v| v.iter().next_back().map(|(_, w)| w))
            .ok_or(ProfileError::UnknownWorkflow(id))
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
            .and_then(|v| v.get(&version))
            .ok_or(ProfileError::UnknownWorkflow(id))
    }

    pub fn list(&self) -> Vec<&WorkflowDefinition> {
        let mut r = Vec::new();
        for v in self.workflows.values() {
            r.extend(v.values());
        }
        r.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| a.version.cmp(&b.version)));
        r
    }
}

// ── Internal helpers ────────────────────────────────────

pub fn workflows_root(repo_root: &Path) -> PathBuf {
    repo_root.join(".knots").join("workflows")
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum BundleFormat {
    Json,
    Toml,
}

pub(crate) fn parse_bundle(
    raw: &str,
    format: BundleFormat,
) -> Result<WorkflowDefinition, ProfileError> {
    match format {
        BundleFormat::Json => parse_bundle_json(raw),
        BundleFormat::Toml => parse_bundle_toml(raw),
    }
}

pub(crate) fn push_unique(items: &mut Vec<String>, value: String) {
    if !items.iter().any(|item| item == &value) {
        items.push(value);
    }
}
