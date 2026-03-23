use std::collections::{BTreeMap, HashMap, HashSet};
use std::error::Error;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::installed_workflows;

const PROFILES_TOML: &str = include_str!("profiles.toml");
const WILDCARD_STATE: &str = "*";

pub const READY_FOR_PLANNING: &str = "ready_for_planning";
pub const PLANNING: &str = "planning";
pub const READY_FOR_PLAN_REVIEW: &str = "ready_for_plan_review";
pub const PLAN_REVIEW: &str = "plan_review";
pub const READY_FOR_IMPLEMENTATION: &str = "ready_for_implementation";
pub const IMPLEMENTATION: &str = "implementation";
pub const READY_FOR_IMPLEMENTATION_REVIEW: &str = "ready_for_implementation_review";
pub const IMPLEMENTATION_REVIEW: &str = "implementation_review";
pub const READY_FOR_SHIPMENT: &str = "ready_for_shipment";
pub const SHIPMENT: &str = "shipment";
pub const READY_FOR_SHIPMENT_REVIEW: &str = "ready_for_shipment_review";
pub const SHIPMENT_REVIEW: &str = "shipment_review";
pub const SHIPPED: &str = "shipped";
pub const DEFERRED: &str = "deferred";
pub const ABANDONED: &str = "abandoned";

const ALL_STATES: [&str; 15] = [
    READY_FOR_PLANNING,
    PLANNING,
    READY_FOR_PLAN_REVIEW,
    PLAN_REVIEW,
    READY_FOR_IMPLEMENTATION,
    IMPLEMENTATION,
    READY_FOR_IMPLEMENTATION_REVIEW,
    IMPLEMENTATION_REVIEW,
    READY_FOR_SHIPMENT,
    SHIPMENT,
    READY_FOR_SHIPMENT_REVIEW,
    SHIPMENT_REVIEW,
    SHIPPED,
    DEFERRED,
    ABANDONED,
];

const PLANNING_STATES: [&str; 4] = [
    READY_FOR_PLANNING,
    PLANNING,
    READY_FOR_PLAN_REVIEW,
    PLAN_REVIEW,
];

const IMPLEMENTATION_REVIEW_STATES: [&str; 2] =
    [READY_FOR_IMPLEMENTATION_REVIEW, IMPLEMENTATION_REVIEW];

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
    pub terminal_states: Vec<String>,
    pub transitions: Vec<WorkflowTransition>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub action_prompts: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub prompt_acceptance: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawProfileFile {
    #[serde(default)]
    profiles: Vec<RawProfileDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawProfileDefinition {
    id: String,
    #[serde(default)]
    description: Option<String>,
    planning_mode: GateMode,
    implementation_review_mode: GateMode,
    output: OutputMode,
    owners: ProfileOwners,
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
            ProfileError::InvalidDefinition(message) => {
                write!(f, "invalid profile definition: {}", message)
            }
            ProfileError::InvalidBundle(message) => {
                write!(f, "invalid workflow bundle: {}", message)
            }
            ProfileError::MissingProfileReference => {
                write!(f, "profile id is required")
            }
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
            ProfileError::Toml(err) => Some(err),
            ProfileError::InvalidDefinition(_) => None,
            ProfileError::InvalidBundle(_) => None,
            ProfileError::MissingProfileReference => None,
            ProfileError::UnknownProfile(_) => None,
            ProfileError::UnknownWorkflow(_) => None,
            ProfileError::UnknownState { .. } => None,
            ProfileError::InvalidTransition(err) => Some(err),
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
    pub fn load() -> Result<Self, ProfileError> {
        Self::from_toml(PROFILES_TOML)
    }

    pub fn load_for_repo(repo_root: &Path) -> Result<Self, ProfileError> {
        let mut registry = Self::from_toml(PROFILES_TOML)?;
        let installed = installed_workflows::InstalledWorkflowRegistry::load(repo_root)?;
        let workflow = installed.current_workflow()?;
        if !workflow.builtin {
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
        let file: RawProfileFile = toml::from_str(raw)?;
        if file.profiles.is_empty() {
            return Err(ProfileError::InvalidDefinition(
                "at least one profile must be defined".to_string(),
            ));
        }

        let mut profiles = HashMap::new();
        let mut aliases = HashMap::new();

        for raw_profile in file.profiles {
            let profile = normalize_profile_definition(raw_profile)?;
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

    /// Returns the owner kind for any workflow state (action or queue).
    ///
    /// Queue states (`ready_for_*`) map to the owner of their
    /// corresponding action state. Terminal states return `None`.
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
        if !self.action_states.is_empty() {
            return self
                .action_states
                .iter()
                .any(|candidate| candidate == state);
        }
        self.owners.for_action_state(state).is_some() || state == "evaluating"
    }

    pub fn prompt_for_action_state(&self, state: &str) -> Option<&str> {
        self.action_prompts.get(state).map(String::as_str)
    }

    pub fn acceptance_for_action_state(&self, state: &str) -> &[String] {
        self.prompt_acceptance
            .get(state)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn is_terminal_state(&self, state: &str) -> bool {
        self.terminal_states
            .iter()
            .any(|candidate| candidate == state)
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
    normalize_scalar(raw)
}

fn builtin_workflow_id() -> String {
    installed_workflows::COMPATIBILITY_WORKFLOW_ID.to_string()
}

fn normalize_profile_definition(
    raw: RawProfileDefinition,
) -> Result<ProfileDefinition, ProfileError> {
    let id = normalize_profile_id(raw.id.as_str())
        .ok_or_else(|| ProfileError::InvalidDefinition("profile id is required".to_string()))?;
    let aliases = legacy_aliases(&id)
        .iter()
        .map(|alias| (*alias).to_string())
        .collect::<Vec<_>>();

    let mut states = ALL_STATES
        .iter()
        .map(|value| (*value).to_string())
        .collect::<Vec<_>>();

    if raw.planning_mode == GateMode::Skipped {
        states.retain(|state| !PLANNING_STATES.contains(&state.as_str()));
    }
    if raw.implementation_review_mode == GateMode::Skipped {
        states.retain(|state| !IMPLEMENTATION_REVIEW_STATES.contains(&state.as_str()));
    }

    let state_set = states.iter().cloned().collect::<HashSet<_>>();
    let mut transitions = canonical_transitions();

    if raw.planning_mode == GateMode::Optional || raw.planning_mode == GateMode::Skipped {
        transitions.push(WorkflowTransition {
            from: READY_FOR_PLANNING.to_string(),
            to: READY_FOR_IMPLEMENTATION.to_string(),
        });
    }

    if raw.implementation_review_mode == GateMode::Optional
        || raw.implementation_review_mode == GateMode::Skipped
    {
        transitions.push(WorkflowTransition {
            from: IMPLEMENTATION.to_string(),
            to: READY_FOR_SHIPMENT.to_string(),
        });
    }

    transitions.retain(|transition| {
        (transition.from == WILDCARD_STATE || state_set.contains(&transition.from))
            && state_set.contains(&transition.to)
    });

    transitions.sort_by(|left, right| {
        left.from
            .cmp(&right.from)
            .then_with(|| left.to.cmp(&right.to))
    });
    transitions.dedup_by(|left, right| left.from == right.from && left.to == right.to);

    let initial_state = if raw.planning_mode == GateMode::Skipped {
        READY_FOR_IMPLEMENTATION.to_string()
    } else {
        READY_FOR_PLANNING.to_string()
    };

    if !state_set.contains(&initial_state) {
        return Err(ProfileError::InvalidDefinition(format!(
            "profile '{}' has invalid initial state '{}'",
            id, initial_state
        )));
    }

    let terminal_states = vec![SHIPPED.to_string(), ABANDONED.to_string()];
    let queue_states = states
        .iter()
        .filter(|state| state.starts_with("ready_for_"))
        .cloned()
        .collect::<Vec<_>>();
    let action_states = states
        .iter()
        .filter(|state| {
            matches!(
                state.as_str(),
                PLANNING
                    | PLAN_REVIEW
                    | IMPLEMENTATION
                    | IMPLEMENTATION_REVIEW
                    | SHIPMENT
                    | SHIPMENT_REVIEW
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut owner_states = BTreeMap::new();
    owner_states.insert(READY_FOR_PLANNING.to_string(), raw.owners.planning.clone());
    owner_states.insert(PLANNING.to_string(), raw.owners.planning.clone());
    owner_states.insert(
        READY_FOR_PLAN_REVIEW.to_string(),
        raw.owners.plan_review.clone(),
    );
    owner_states.insert(PLAN_REVIEW.to_string(), raw.owners.plan_review.clone());
    owner_states.insert(
        READY_FOR_IMPLEMENTATION.to_string(),
        raw.owners.implementation.clone(),
    );
    owner_states.insert(
        IMPLEMENTATION.to_string(),
        raw.owners.implementation.clone(),
    );
    owner_states.insert(
        READY_FOR_IMPLEMENTATION_REVIEW.to_string(),
        raw.owners.implementation_review.clone(),
    );
    owner_states.insert(
        IMPLEMENTATION_REVIEW.to_string(),
        raw.owners.implementation_review.clone(),
    );
    owner_states.insert(READY_FOR_SHIPMENT.to_string(), raw.owners.shipment.clone());
    owner_states.insert(SHIPMENT.to_string(), raw.owners.shipment.clone());
    owner_states.insert(
        READY_FOR_SHIPMENT_REVIEW.to_string(),
        raw.owners.shipment_review.clone(),
    );
    owner_states.insert(
        SHIPMENT_REVIEW.to_string(),
        raw.owners.shipment_review.clone(),
    );
    let owners = ProfileOwners {
        states: owner_states,
        ..raw.owners
    };

    Ok(ProfileDefinition {
        id,
        workflow_id: builtin_workflow_id(),
        aliases,
        description: raw
            .description
            .and_then(|value| normalize_scalar(value.as_str())),
        planning_mode: raw.planning_mode,
        implementation_review_mode: raw.implementation_review_mode,
        outputs: outputs_from_legacy_mode(&raw.output, &action_states),
        owners,
        initial_state,
        states,
        queue_states,
        action_states,
        terminal_states,
        transitions,
        action_prompts: BTreeMap::new(),
        prompt_acceptance: BTreeMap::new(),
    })
}

fn outputs_from_legacy_mode(
    mode: &OutputMode,
    action_states: &[String],
) -> BTreeMap<String, ActionOutputDef> {
    let artifact_type = match mode {
        OutputMode::Local => "local",
        OutputMode::Remote => "remote",
        OutputMode::Pr => "pr",
        OutputMode::RemoteMain => "remote_main",
    };
    action_states
        .iter()
        .map(|state| {
            (
                state.clone(),
                ActionOutputDef {
                    artifact_type: artifact_type.to_string(),
                    access_hint: None,
                },
            )
        })
        .collect()
}

fn normalize_scalar(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_ascii_lowercase())
    }
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

fn canonical_transitions() -> Vec<WorkflowTransition> {
    vec![
        transition(READY_FOR_PLANNING, PLANNING),
        transition(PLANNING, READY_FOR_PLAN_REVIEW),
        transition(READY_FOR_PLAN_REVIEW, PLAN_REVIEW),
        transition(PLAN_REVIEW, READY_FOR_IMPLEMENTATION),
        transition(PLAN_REVIEW, READY_FOR_PLANNING),
        transition(READY_FOR_IMPLEMENTATION, IMPLEMENTATION),
        transition(IMPLEMENTATION, READY_FOR_IMPLEMENTATION_REVIEW),
        transition(READY_FOR_IMPLEMENTATION_REVIEW, IMPLEMENTATION_REVIEW),
        transition(IMPLEMENTATION_REVIEW, READY_FOR_SHIPMENT),
        transition(IMPLEMENTATION_REVIEW, READY_FOR_IMPLEMENTATION),
        transition(READY_FOR_SHIPMENT, SHIPMENT),
        transition(SHIPMENT, READY_FOR_SHIPMENT_REVIEW),
        transition(READY_FOR_SHIPMENT_REVIEW, SHIPMENT_REVIEW),
        transition(SHIPMENT_REVIEW, SHIPPED),
        transition(SHIPMENT_REVIEW, READY_FOR_IMPLEMENTATION),
        transition(SHIPMENT_REVIEW, READY_FOR_SHIPMENT),
        transition(WILDCARD_STATE, DEFERRED),
        transition(WILDCARD_STATE, ABANDONED),
    ]
}

fn transition(from: &str, to: &str) -> WorkflowTransition {
    WorkflowTransition {
        from: from.to_string(),
        to: to.to_string(),
    }
}

fn legacy_aliases(id: &str) -> &'static [&'static str] {
    match id {
        "autopilot" => &[
            "automation_granular",
            "default",
            "delivery",
            "automation",
            "granular",
        ],
        "semiauto" => &["human_gate", "human", "coarse", "pr_human_gate"],
        _ => &[],
    }
}

#[cfg(test)]
#[path = "profile_tests.rs"]
mod tests;
