use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::{error::Error, fmt};

use serde::{Deserialize, Serialize};

use crate::installed_workflows;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowTransition {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GateMode {
    Required,
    Optional,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionOutputDef {
    pub artifact_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepMetadata {
    pub action_state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StepOwner>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<ActionOutputDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OwnerKind {
    Human,
    Agent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepOwner {
    pub kind: OwnerKind,
    #[serde(default)]
    pub agent_name: Option<String>,
    #[serde(default)]
    pub agent_model: Option<String>,
    #[serde(default)]
    pub agent_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileOwners {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planning: Option<StepOwner>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_review: Option<StepOwner>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implementation: Option<StepOwner>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implementation_review: Option<StepOwner>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shipment: Option<StepOwner>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shipment_review: Option<StepOwner>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub states: BTreeMap<String, StepOwner>,
}

const ACTION_OWNER_KEYS: [&str; 6] = [
    "planning",
    "plan_review",
    "implementation",
    "implementation_review",
    "shipment",
    "shipment_review",
];

impl ProfileOwners {
    pub fn from_states(states: BTreeMap<String, StepOwner>) -> Self {
        let get = |key: &str| states.get(key).cloned();
        Self {
            planning: get(ACTION_OWNER_KEYS[0]),
            plan_review: get(ACTION_OWNER_KEYS[1]),
            implementation: get(ACTION_OWNER_KEYS[2]),
            implementation_review: get(ACTION_OWNER_KEYS[3]),
            shipment: get(ACTION_OWNER_KEYS[4]),
            shipment_review: get(ACTION_OWNER_KEYS[5]),
            states,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileDefinition {
    pub id: String,
    #[serde(default = "builtin_workflow_id")]
    pub workflow_id: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub planning_mode: GateMode,
    pub implementation_review_mode: GateMode,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub outputs: BTreeMap<String, ActionOutputDef>,
    pub owners: ProfileOwners,
    pub initial_state: String,
    pub states: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queue_states: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub action_states: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub queue_actions: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub action_kinds: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub escape_states: Vec<String>,
    pub terminal_states: Vec<String>,
    pub transitions: Vec<WorkflowTransition>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub action_prompts: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub prompt_acceptance: BTreeMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub review_hints: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ProfileRegistry {
    profiles: HashMap<String, ProfileDefinition>,
    aliases: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidWorkflowTransition {
    pub profile_id: String,
    pub from: String,
    pub to: String,
}

impl fmt::Display for InvalidWorkflowTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid state transition in profile '{}': {} -> {}",
            self.profile_id, self.from, self.to
        )
    }
}
impl Error for InvalidWorkflowTransition {}

#[derive(Debug)]
pub enum ProfileError {
    Toml(toml::de::Error),
    #[cfg_attr(not(test), allow(dead_code))]
    InvalidDefinition(String),
    InvalidBundle(String),
    MissingProfileReference,
    UnknownProfile(String),
    UnknownWorkflow(String),
    UnknownState {
        profile_id: String,
        state: String,
    },
    InvalidTransition(InvalidWorkflowTransition),
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProfileError::Toml(err) => write!(f, "invalid profile TOML: {}", err),
            ProfileError::InvalidDefinition(m) => write!(f, "invalid profile definition: {m}"),
            ProfileError::InvalidBundle(m) => write!(f, "invalid workflow bundle: {m}"),
            ProfileError::MissingProfileReference => write!(f, "profile id is required"),
            ProfileError::UnknownProfile(id) => write!(f, "unknown profile '{}'", id),
            ProfileError::UnknownWorkflow(id) => write!(f, "unknown workflow '{}'", id),
            ProfileError::UnknownState { profile_id, state } => {
                write!(f, "unknown state '{}' for profile '{}'", state, profile_id)
            }
            ProfileError::InvalidTransition(err) => write!(f, "{}", err),
        }
    }
}

impl Error for ProfileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ProfileError::Toml(e) => Some(e),
            ProfileError::InvalidTransition(e) => Some(e),
            _ => None,
        }
    }
}

impl From<toml::de::Error> for ProfileError {
    fn from(value: toml::de::Error) -> Self {
        ProfileError::Toml(value)
    }
}

impl From<InvalidWorkflowTransition> for ProfileError {
    fn from(value: InvalidWorkflowTransition) -> Self {
        ProfileError::InvalidTransition(value)
    }
}

impl ProfileRegistry {
    #[cfg(test)]
    pub fn load() -> Result<Self, ProfileError> {
        let mut registry = Self::empty();
        for (_knot_type, builtin) in installed_workflows::builtin::builtin_workflows()? {
            for profile in builtin.list_profiles() {
                registry.insert_builtin_profile(profile);
            }
        }
        Ok(registry)
    }

    pub fn load_for_repo(repo_root: &Path) -> Result<Self, ProfileError> {
        let mut registry = Self::empty();
        let installed = installed_workflows::InstalledWorkflowRegistry::load(repo_root)?;
        for workflow in installed.list() {
            if workflow.builtin {
                for profile in workflow.list_profiles() {
                    registry.insert_builtin_profile(profile);
                }
                continue;
            }
            for mut profile in workflow.list_profiles() {
                let raw_id = profile.id.clone();
                let namespaced = installed_workflows::namespaced_profile_id(&workflow.id, &raw_id);
                profile.aliases.push(raw_id);
                profile.id = namespaced.clone();
                registry
                    .aliases
                    .insert(namespaced.clone(), namespaced.clone());
                registry.profiles.insert(namespaced, profile);
            }
        }
        Ok(registry)
    }

    fn empty() -> Self {
        Self {
            profiles: HashMap::new(),
            aliases: HashMap::new(),
        }
    }

    fn insert_builtin_profile(&mut self, profile: ProfileDefinition) {
        let canonical_id = profile.id.clone();
        self.aliases
            .insert(canonical_id.clone(), canonical_id.clone());
        self.profiles.insert(canonical_id, profile);
    }

    pub fn list(&self) -> Vec<ProfileDefinition> {
        let mut values = self.profiles.values().cloned().collect::<Vec<_>>();
        values.sort_by(|left, right| left.id.cmp(&right.id));
        values
    }

    pub fn resolve(&self, profile_id: Option<&str>) -> Result<&ProfileDefinition, ProfileError> {
        let id = profile_id
            .and_then(normalize_profile_id)
            .ok_or(ProfileError::MissingProfileReference)?;
        self.lookup(&id).ok_or(ProfileError::UnknownProfile(id))
    }

    pub fn require(&self, profile_id: &str) -> Result<&ProfileDefinition, ProfileError> {
        let id = normalize_profile_id(profile_id)
            .ok_or_else(|| ProfileError::UnknownProfile(profile_id.to_string()))?;
        self.lookup(&id).ok_or(ProfileError::UnknownProfile(id))
    }

    fn lookup(&self, normalized_id: &str) -> Option<&ProfileDefinition> {
        if let Some(profile) = self.profiles.get(normalized_id) {
            return Some(profile);
        }

        let canonical = self.aliases.get(normalized_id)?;
        self.profiles.get(canonical)
    }
}

pub fn normalize_profile_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_ascii_lowercase())
    }
}

fn builtin_workflow_id() -> String {
    installed_workflows::builtin_workflow_id_for_knot_type(crate::domain::knot_type::KnotType::Work)
}

#[cfg(test)]
#[path = "profile_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "profile_tests_exploration.rs"]
mod tests_exploration;
#[cfg(test)]
#[path = "profile_tests_installed_workflows.rs"]
mod tests_installed_workflows;
