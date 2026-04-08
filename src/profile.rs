use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::{error::Error, fmt};

use serde::{Deserialize, Serialize};

use crate::installed_workflows;
pub use crate::profile_consts::*;

const PROFILES_TOML: &str = include_str!("profiles.toml");
const WILDCARD_STATE: &str = "*";

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
pub enum OutputMode {
    Local,
    Remote,
    Pr,
    RemoteMain,
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
    pub planning: StepOwner,
    pub plan_review: StepOwner,
    pub implementation: StepOwner,
    pub implementation_review: StepOwner,
    pub shipment: StepOwner,
    pub shipment_review: StepOwner,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub states: BTreeMap<String, StepOwner>,
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
    InvalidDefinition(String),
    InvalidBundle(String),
    MissingProfileReference,
    UnknownProfile(String),
    UnknownWorkflow(String),
    UnknownState { profile_id: String, state: String },
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
        let mut registry = Self::from_toml(PROFILES_TOML)?;
        let builtin = installed_workflows::builtin::knots_sdlc_workflow_for_test()?;
        for profile in builtin.list_profiles() {
            if !registry.profiles.contains_key(&profile.id) {
                registry
                    .aliases
                    .insert(profile.id.clone(), profile.id.clone());
                registry.profiles.insert(profile.id.clone(), profile);
            }
        }
        Ok(registry)
    }

    pub fn load_for_repo(repo_root: &Path) -> Result<Self, ProfileError> {
        let mut registry = Self::from_toml(PROFILES_TOML)?;
        let installed = installed_workflows::InstalledWorkflowRegistry::load(repo_root)?;
        for workflow in installed.list() {
            if workflow.builtin {
                for profile in workflow.list_profiles() {
                    if let Some(existing) = registry.profiles.get_mut(&profile.id) {
                        existing.action_prompts = profile.action_prompts.clone();
                        existing.prompt_acceptance = profile.prompt_acceptance.clone();
                    } else {
                        registry
                            .aliases
                            .insert(profile.id.clone(), profile.id.clone());
                        registry.profiles.insert(profile.id.clone(), profile);
                    }
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

    pub(crate) fn from_toml(raw: &str) -> Result<Self, ProfileError> {
        let file: crate::profile_normalize::RawProfileFile = toml::from_str(raw)?;
        if file.profiles.is_empty() {
            return Err(ProfileError::InvalidDefinition(
                "at least one profile must be defined".to_string(),
            ));
        }

        let mut profiles = HashMap::new();
        let mut aliases = HashMap::new();

        for raw_profile in file.profiles {
            let profile = crate::profile_normalize::normalize(raw_profile)?;
            if profiles
                .insert(profile.id.clone(), profile.clone())
                .is_some()
            {
                return Err(ProfileError::InvalidDefinition(
                    "duplicate profile id in profile file".to_string(),
                ));
            }
            for alias in &profile.aliases {
                aliases.insert(alias.clone(), profile.id.clone());
            }
        }

        Ok(Self { profiles, aliases })
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

impl ProfileOwners {
    pub fn for_action_state(&self, state: &str) -> Option<&StepOwner> {
        match state {
            PLANNING => Some(&self.planning),
            PLAN_REVIEW => Some(&self.plan_review),
            IMPLEMENTATION => Some(&self.implementation),
            IMPLEMENTATION_REVIEW => Some(&self.implementation_review),
            SHIPMENT => Some(&self.shipment),
            SHIPMENT_REVIEW => Some(&self.shipment_review),
            _ => None,
        }
    }

    pub fn owner_kind_for_state(&self, state: &str) -> Option<&OwnerKind> {
        if let Some(owner) = self.states.get(state) {
            return Some(&owner.kind);
        }
        let action = match state {
            READY_FOR_PLANNING | PLANNING => PLANNING,
            READY_FOR_PLAN_REVIEW | PLAN_REVIEW => PLAN_REVIEW,
            READY_FOR_IMPLEMENTATION | IMPLEMENTATION => IMPLEMENTATION,
            READY_FOR_IMPLEMENTATION_REVIEW | IMPLEMENTATION_REVIEW => IMPLEMENTATION_REVIEW,
            READY_FOR_SHIPMENT | SHIPMENT => SHIPMENT,
            READY_FOR_SHIPMENT_REVIEW | SHIPMENT_REVIEW => SHIPMENT_REVIEW,
            _ => return None,
        };
        self.for_action_state(action).map(|o| &o.kind)
    }
}

impl ProfileDefinition {
    pub fn is_queue_state(&self, state: &str) -> bool {
        if !self.queue_states.is_empty() {
            return self.queue_states.iter().any(|candidate| candidate == state);
        }
        state.starts_with("ready_for_") || state == "ready_to_evaluate"
    }

    #[allow(dead_code)]
    pub fn is_action_state(&self, state: &str) -> bool {
        if self.is_escape_state(state) {
            return false;
        }
        if !self.action_states.is_empty() {
            return self
                .action_states
                .iter()
                .any(|candidate| candidate == state);
        }
        self.owners.for_action_state(state).is_some() || state == "evaluating"
    }

    pub fn action_for_queue_state(&self, state: &str) -> Option<&str> {
        self.queue_actions.get(state).map(String::as_str)
    }

    pub fn is_gate_action_state(&self, state: &str) -> bool {
        matches!(
            self.action_kinds.get(state).map(String::as_str),
            Some("gate") | Some("review")
        )
    }

    pub fn is_escape_state(&self, state: &str) -> bool {
        let s = normalize_state_alias(state);
        self.escape_states.iter().any(|c| c == s)
    }

    pub fn prompt_for_action_state(&self, s: &str) -> Option<&str> {
        self.action_prompts.get(s).map(String::as_str)
    }

    pub fn acceptance_for_action_state(&self, s: &str) -> &[String] {
        self.prompt_acceptance
            .get(s)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn step_metadata_for(&self, action_state: &str) -> StepMetadata {
        let owner = self
            .owners
            .states
            .get(action_state)
            .or_else(|| self.owners.for_action_state(action_state))
            .cloned();
        StepMetadata {
            action_state: action_state.to_string(),
            action_kind: self.action_kinds.get(action_state).cloned(),
            owner,
            output: self.outputs.get(action_state).cloned(),
            review_hint: self.review_hints.get(action_state).cloned(),
        }
    }

    pub fn is_terminal_state(&self, state: &str) -> bool {
        self.terminal_states.iter().any(|c| c == state)
    }

    pub fn require_state(&self, state: &str) -> Result<(), ProfileError> {
        let normalized = normalize_state_alias(state);
        if self.states.iter().any(|candidate| candidate == normalized) {
            return Ok(());
        }
        Err(ProfileError::UnknownState {
            profile_id: self.id.clone(),
            state: normalized.to_string(),
        })
    }

    pub fn validate_transition(
        &self,
        from: &str,
        to: &str,
        force: bool,
    ) -> Result<(), ProfileError> {
        let from = normalize_state_alias(from);
        let to = normalize_state_alias(to);
        self.require_state(from)?;
        self.require_state(to)?;

        if force || from == to {
            return Ok(());
        }

        let allowed = self.transitions.iter().any(|transition| {
            (transition.from == from || transition.from == WILDCARD_STATE) && transition.to == to
        });
        if allowed {
            return Ok(());
        }

        Err(InvalidWorkflowTransition {
            profile_id: self.id.clone(),
            from: from.to_string(),
            to: to.to_string(),
        }
        .into())
    }

    pub fn next_happy_path_state(&self, current: &str) -> Option<&str> {
        let current = normalize_state_alias(current);
        let pos = self.states.iter().position(|state| state == current)?;
        for candidate in &self.states[pos + 1..] {
            let valid = self
                .transitions
                .iter()
                .any(|transition| transition.from == current && transition.to == *candidate);
            if valid {
                return Some(candidate.as_str());
            }
        }
        None
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
    installed_workflows::BUILTIN_WORKFLOW_ID.to_string()
}

fn normalize_state_alias(raw: &str) -> &str {
    match raw.trim() {
        "idea" => READY_FOR_PLANNING,
        "work_item" => READY_FOR_IMPLEMENTATION,
        "implementing" => IMPLEMENTATION,
        "implemented" => READY_FOR_IMPLEMENTATION_REVIEW,
        "reviewing" => IMPLEMENTATION_REVIEW,
        "rejected" | "refining" => READY_FOR_IMPLEMENTATION,
        "approved" => READY_FOR_SHIPMENT,
        other => other,
    }
}

#[cfg(test)]
#[path = "profile_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "profile_tests_exploration.rs"]
mod tests_exploration;
