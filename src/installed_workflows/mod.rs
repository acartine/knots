pub(crate) mod builtin;
pub(crate) mod bundle_json;
pub(crate) mod bundle_toml;
mod ids;
mod knot_type_registry;
mod loader;
mod operations;
pub(crate) mod profile_json;
pub(crate) mod profile_toml;
mod registry;

#[cfg(test)]
mod tests_helpers;
#[cfg(test)]
mod tests_output_sections;
#[cfg(test)]
mod tests_owner_projection;
#[cfg(test)]
mod tests_parsing;
#[cfg(test)]
mod tests_parsing_outputs;
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
pub use knot_type_registry::{KnotTypeWorkflowConfig, WorkflowRef};
pub use registry::InstalledWorkflowRegistry;

pub use ids::normalize_workflow_id;
#[cfg(test)]
pub use operations::write_repo_config;
pub use operations::{
    ensure_builtin_workflows_registered, install_bundle, namespaced_profile_id, read_repo_config,
    register_workflow_for_knot_type, set_current_workflow_selection,
    set_current_workflow_selection_for_knot_type, set_workflow_default_profile,
};

const DEFAULT_BUNDLE_FILE: &str = "bundle.json";
const TOML_BUNDLE_FILE: &str = "bundle.toml";

pub fn builtin_workflow_id_for_knot_type(knot_type: crate::domain::knot_type::KnotType) -> String {
    builtin::builtin_workflow_ref(knot_type).workflow_id
}

pub fn is_builtin_workflow_id(workflow_id: &str) -> bool {
    let workflow_id = normalize_workflow_id(workflow_id);
    crate::domain::knot_type::KnotType::ALL
        .into_iter()
        .any(|knot_type| builtin_workflow_id_for_knot_type(knot_type) == workflow_id)
}

// ── WorkflowRepoConfig ─────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowRepoConfig {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub knot_type_workflows: BTreeMap<String, KnotTypeWorkflowConfig>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub default_profiles: BTreeMap<String, String>,
}

impl WorkflowRepoConfig {
    pub(crate) fn normalize(mut self) -> Self {
        self.knot_type_workflows =
            knot_type_registry::normalize_knot_type_workflows(self.knot_type_workflows);
        self
    }

    #[cfg(test)]
    pub(crate) fn current_profile_id(&self) -> Option<&str> {
        let workflow_id = self
            .current_workflow_ref_for_knot_type(crate::domain::knot_type::KnotType::Work)?
            .workflow_id;
        self.default_profile_id_for_workflow(&workflow_id)
    }

    pub(crate) fn set_default_profile(&mut self, workflow_id: &str, profile_id: String) {
        self.default_profiles
            .insert(workflow_id.to_string(), profile_id);
    }

    pub(crate) fn default_profile_id_for_workflow(&self, workflow_id: &str) -> Option<&str> {
        let workflow_id = normalize_workflow_id(workflow_id);
        self.default_profiles
            .get(workflow_id.as_str())
            .map(String::as_str)
    }

    pub fn current_workflow_ref_for_knot_type(
        &self,
        knot_type: crate::domain::knot_type::KnotType,
    ) -> Option<WorkflowRef> {
        self.knot_type_workflows
            .get(knot_type.as_str())
            .map(|entry| entry.default.clone())
    }

    pub fn register_workflow_for_knot_type(
        &mut self,
        knot_type: crate::domain::knot_type::KnotType,
        workflow: WorkflowRef,
        set_default: bool,
    ) {
        let normalized = workflow.normalize();
        let entry = self
            .knot_type_workflows
            .entry(knot_type.as_str().to_string())
            .or_insert_with(|| KnotTypeWorkflowConfig {
                default: normalized.clone(),
                registered: Vec::new(),
            });
        entry.register(normalized.clone());
        if set_default {
            entry.default = normalized;
        }
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

pub(crate) fn build_prompt_params(
    workflow_id: &str,
    profile: &crate::profile::ProfileDefinition,
    prompt: &PromptDefinition,
) -> BTreeMap<String, String> {
    let mut params = BTreeMap::new();
    params.insert("workflow_id".to_string(), workflow_id.to_string());
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

pub(crate) fn render_prompt_template(
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

fn parse_output_specific_line(line: &str) -> Option<(&str, &str, usize)> {
    let trimmed = line.trim_start();
    let indent_len = line.len() - trimmed.len();
    let rest = trimmed.strip_prefix("`{{ output }}` = `")?;
    let (mode, message) = rest.split_once("` means ")?;
    Some((mode, message, indent_len))
}

fn starts_with_list_marker(line: &str) -> bool {
    line.starts_with("- ")
        || line.starts_with("* ")
        || line
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_digit() && line.contains(". "))
}

fn resolve_output_specific_sections(template: &str, output_mode: Option<&str>) -> String {
    let lines = template.lines().collect::<Vec<_>>();
    let mut resolved = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let Some((mode, message, indent_len)) = parse_output_specific_line(line) else {
            resolved.push(line.to_string());
            index += 1;
            continue;
        };

        let mut block = vec![format!("{}{}", " ".repeat(indent_len), message)];
        index += 1;
        while index < lines.len() {
            let next = lines[index];
            let next_trimmed = next.trim_start();
            let next_indent_len = next.len() - next_trimmed.len();
            if next_trimmed.is_empty()
                || parse_output_specific_line(next).is_some()
                || next_indent_len < indent_len
                || starts_with_list_marker(next_trimmed)
            {
                break;
            }
            block.push(next.to_string());
            index += 1;
        }

        if output_mode == Some(mode) {
            resolved.extend(block);
        }
    }
    resolved.join("\n")
}

pub(crate) fn render_prompt_body(
    workflow_id: &str,
    profile: &crate::profile::ProfileDefinition,
    prompt: &PromptDefinition,
) -> String {
    let params = build_prompt_params(workflow_id, profile, prompt);
    let resolved_template =
        resolve_output_specific_sections(&prompt.body, params.get("output").map(String::as_str));
    let mut unresolved = Vec::new();
    render_prompt_template(&resolved_template, &params, &mut unresolved)
}

#[cfg(test)]
impl PromptDefinition {
    pub fn render(&self, workflow: &WorkflowDefinition, profile: &ProfileDefinition) -> String {
        let params = build_prompt_params(&workflow.id, profile, self);
        let template =
            resolve_output_specific_sections(&self.body, params.get("output").map(String::as_str));
        let mut unresolved = Vec::new();
        let mut body = render_prompt_template(&template, &params, &mut unresolved);
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

// ── Internal helpers ────────────────────────────────────

pub fn workflows_root(repo_root: &Path) -> PathBuf {
    if repo_root.join(".git").exists() {
        repo_root.join(".knots").join("workflows")
    } else if repo_root.join("cache").exists()
        || repo_root.join("events").exists()
        || repo_root.join("index").exists()
        || repo_root.join("locks").exists()
    {
        repo_root.join("workflows")
    } else {
        repo_root.join(".knots").join("workflows")
    }
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
