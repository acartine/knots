use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const WORKFLOW_FILE_PATH: &str = ".knots/workflows.toml";
const WILDCARD_STATE: &str = "*";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowTransition {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowDefinition {
    pub id: String,
    #[serde(default)]
    pub description: Option<String>,
    pub initial_state: String,
    pub states: Vec<String>,
    pub terminal_states: Vec<String>,
    pub transitions: Vec<WorkflowTransition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkflowFile {
    #[serde(default)]
    workflows: Vec<WorkflowDefinition>,
}

#[derive(Debug, Clone)]
pub struct WorkflowRegistry {
    workflows: HashMap<String, WorkflowDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidWorkflowTransition {
    pub workflow_id: String,
    pub from: String,
    pub to: String,
}

impl fmt::Display for InvalidWorkflowTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid state transition in workflow '{}': {} -> {}",
            self.workflow_id, self.from, self.to
        )
    }
}

impl Error for InvalidWorkflowTransition {}

#[derive(Debug)]
pub enum WorkflowError {
    Io(std::io::Error),
    Toml(toml::de::Error),
    MissingFile(PathBuf),
    InvalidDefinition(String),
    MissingWorkflowReference,
    UnknownWorkflow(String),
    UnknownState { workflow_id: String, state: String },
    InvalidTransition(InvalidWorkflowTransition),
}

impl fmt::Display for WorkflowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkflowError::Io(err) => write!(f, "I/O error: {}", err),
            WorkflowError::Toml(err) => write!(f, "invalid workflow TOML: {}", err),
            WorkflowError::MissingFile(path) => {
                write!(f, "workflow file is required: {}", path.display())
            }
            WorkflowError::InvalidDefinition(message) => {
                write!(f, "invalid workflow definition: {}", message)
            }
            WorkflowError::MissingWorkflowReference => {
                write!(f, "workflow id is required")
            }
            WorkflowError::UnknownWorkflow(id) => write!(f, "unknown workflow '{}'", id),
            WorkflowError::UnknownState { workflow_id, state } => {
                write!(
                    f,
                    "unknown state '{}' for workflow '{}'",
                    state, workflow_id
                )
            }
            WorkflowError::InvalidTransition(err) => write!(f, "{}", err),
        }
    }
}

impl Error for WorkflowError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            WorkflowError::Io(err) => Some(err),
            WorkflowError::Toml(err) => Some(err),
            WorkflowError::InvalidDefinition(_) => None,
            WorkflowError::MissingFile(_) => None,
            WorkflowError::MissingWorkflowReference => None,
            WorkflowError::UnknownWorkflow(_) => None,
            WorkflowError::UnknownState { .. } => None,
            WorkflowError::InvalidTransition(err) => Some(err),
        }
    }
}

impl From<std::io::Error> for WorkflowError {
    fn from(value: std::io::Error) -> Self {
        WorkflowError::Io(value)
    }
}

impl From<toml::de::Error> for WorkflowError {
    fn from(value: toml::de::Error) -> Self {
        WorkflowError::Toml(value)
    }
}

impl From<InvalidWorkflowTransition> for WorkflowError {
    fn from(value: InvalidWorkflowTransition) -> Self {
        WorkflowError::InvalidTransition(value)
    }
}

impl WorkflowRegistry {
    pub fn load(repo_root: &Path) -> Result<Self, WorkflowError> {
        let config_path = repo_root.join(WORKFLOW_FILE_PATH);
        if !config_path.exists() {
            return Err(WorkflowError::MissingFile(config_path));
        }

        let raw = std::fs::read_to_string(&config_path)?;
        let file: WorkflowFile = toml::from_str(&raw)?;
        if file.workflows.is_empty() {
            return Err(WorkflowError::InvalidDefinition(
                "at least one workflow must be defined".to_string(),
            ));
        }

        let mut workflows = HashMap::new();
        for workflow in file.workflows {
            let normalized = normalize_definition(workflow)?;
            if workflows
                .insert(normalized.id.clone(), normalized)
                .is_some()
            {
                return Err(WorkflowError::InvalidDefinition(
                    "duplicate workflow id in workflow file".to_string(),
                ));
            }
        }

        Ok(Self { workflows })
    }

    pub fn list(&self) -> Vec<WorkflowDefinition> {
        let mut values = self.workflows.values().cloned().collect::<Vec<_>>();
        values.sort_by(|left, right| left.id.cmp(&right.id));
        values
    }

    pub fn resolve(&self, workflow_id: Option<&str>) -> Result<&WorkflowDefinition, WorkflowError> {
        let id = workflow_id
            .and_then(normalize_scalar)
            .ok_or(WorkflowError::MissingWorkflowReference)?;
        self.workflows
            .get(&id)
            .ok_or_else(|| WorkflowError::UnknownWorkflow(id))
    }

    pub fn require(&self, workflow_id: &str) -> Result<&WorkflowDefinition, WorkflowError> {
        let id = normalize_scalar(workflow_id)
            .ok_or_else(|| WorkflowError::UnknownWorkflow(workflow_id.to_string()))?;
        self.workflows
            .get(&id)
            .ok_or_else(|| WorkflowError::UnknownWorkflow(id))
    }
}

impl WorkflowDefinition {
    pub fn is_terminal_state(&self, state: &str) -> bool {
        self.terminal_states
            .iter()
            .any(|candidate| candidate == state)
    }

    pub fn require_state(&self, state: &str) -> Result<(), WorkflowError> {
        if self.states.iter().any(|candidate| candidate == state) {
            return Ok(());
        }
        Err(WorkflowError::UnknownState {
            workflow_id: self.id.clone(),
            state: state.to_string(),
        })
    }

    pub fn validate_transition(
        &self,
        from: &str,
        to: &str,
        force: bool,
    ) -> Result<(), WorkflowError> {
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
            workflow_id: self.id.clone(),
            from: from.to_string(),
            to: to.to_string(),
        }
        .into())
    }
}

fn normalize_definition(raw: WorkflowDefinition) -> Result<WorkflowDefinition, WorkflowError> {
    let id = normalize_scalar(raw.id.as_str())
        .ok_or_else(|| WorkflowError::InvalidDefinition("workflow id is required".to_string()))?;

    let states = normalize_states(raw.states);
    if states.is_empty() {
        return Err(WorkflowError::InvalidDefinition(format!(
            "workflow '{}' must include at least one state",
            id
        )));
    }

    let state_set = states.iter().cloned().collect::<HashSet<_>>();
    let initial_state = normalize_scalar(raw.initial_state.as_str()).ok_or_else(|| {
        WorkflowError::InvalidDefinition(format!("workflow '{}' must include an initial_state", id))
    })?;
    if !state_set.contains(&initial_state) {
        return Err(WorkflowError::InvalidDefinition(format!(
            "workflow '{}' initial_state '{}' is not in states",
            id, initial_state
        )));
    }

    let mut terminal_states = normalize_states(raw.terminal_states);
    if terminal_states.is_empty() {
        return Err(WorkflowError::InvalidDefinition(format!(
            "workflow '{}' must include at least one terminal state",
            id
        )));
    }

    for state in &terminal_states {
        if !state_set.contains(state) {
            return Err(WorkflowError::InvalidDefinition(format!(
                "workflow '{}' terminal state '{}' is not in states",
                id, state
            )));
        }
    }
    terminal_states.sort();
    terminal_states.dedup();

    let mut transitions = Vec::new();
    let mut seen = HashSet::new();
    for entry in raw.transitions {
        let from = normalize_scalar(entry.from.as_str()).ok_or_else(|| {
            WorkflowError::InvalidDefinition(format!(
                "workflow '{}' transition.from is required",
                id
            ))
        })?;
        let to = normalize_scalar(entry.to.as_str()).ok_or_else(|| {
            WorkflowError::InvalidDefinition(format!("workflow '{}' transition.to is required", id))
        })?;

        if from != WILDCARD_STATE && !state_set.contains(&from) {
            return Err(WorkflowError::InvalidDefinition(format!(
                "workflow '{}' transition from '{}' not in states",
                id, from
            )));
        }
        if !state_set.contains(&to) {
            return Err(WorkflowError::InvalidDefinition(format!(
                "workflow '{}' transition to '{}' not in states",
                id, to
            )));
        }

        if seen.insert((from.clone(), to.clone())) {
            transitions.push(WorkflowTransition { from, to });
        }
    }

    if transitions.is_empty() {
        return Err(WorkflowError::InvalidDefinition(format!(
            "workflow '{}' must define at least one transition",
            id
        )));
    }

    Ok(WorkflowDefinition {
        id,
        description: raw
            .description
            .and_then(|value| normalize_scalar(value.as_str())),
        initial_state,
        states,
        terminal_states,
        transitions,
    })
}

fn normalize_scalar(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_ascii_lowercase())
    }
}

fn normalize_states(values: Vec<String>) -> Vec<String> {
    let mut states = values
        .into_iter()
        .filter_map(|value| normalize_scalar(&value))
        .collect::<Vec<_>>();
    states.sort();
    states.dedup();
    states
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{WorkflowDefinition, WorkflowRegistry, WorkflowTransition};

    fn repo_root() -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("knots-workflow-test-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(root.join(".knots"))
            .expect("workflow test workspace should be creatable");
        root
    }

    #[test]
    fn fails_when_workflow_file_missing() {
        let root = repo_root();
        let result = WorkflowRegistry::load(&root);
        assert!(result.is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn validates_transitions_with_wildcard_and_force() {
        let workflow = WorkflowDefinition {
            id: "sample".to_string(),
            description: None,
            initial_state: "idea".to_string(),
            states: vec![
                "idea".to_string(),
                "work_item".to_string(),
                "abandoned".to_string(),
            ],
            terminal_states: vec!["abandoned".to_string()],
            transitions: vec![
                WorkflowTransition {
                    from: "idea".to_string(),
                    to: "work_item".to_string(),
                },
                WorkflowTransition {
                    from: "*".to_string(),
                    to: "abandoned".to_string(),
                },
            ],
        };

        assert!(workflow
            .validate_transition("idea", "work_item", false)
            .is_ok());
        assert!(workflow
            .validate_transition("work_item", "abandoned", false)
            .is_ok());
        assert!(workflow
            .validate_transition("work_item", "idea", false)
            .is_err());
        assert!(workflow
            .validate_transition("work_item", "idea", true)
            .is_ok());
    }

    #[test]
    fn loads_simple_workflow_from_repo_root() {
        let root = repo_root();
        let path = root.join(".knots/workflows.toml");
        std::fs::write(
            &path,
            concat!(
                "[[workflows]]\n",
                "id = \"simple\"\n",
                "description = \"Traditional linear delivery workflow\"\n",
                "initial_state = \"work_item\"\n",
                "states = [\n",
                "  \"work_item\",\n",
                "  \"implementing\",\n",
                "  \"queued_for_review\",\n",
                "  \"reviewing\",\n",
                "  \"reviewed\",\n",
                "  \"shipping\",\n",
                "  \"shipped\",\n",
                "]\n",
                "terminal_states = [\"shipped\"]\n",
                "\n",
                "[[workflows.transitions]]\n",
                "from = \"work_item\"\n",
                "to = \"implementing\"\n",
                "\n",
                "[[workflows.transitions]]\n",
                "from = \"implementing\"\n",
                "to = \"queued_for_review\"\n",
                "\n",
                "[[workflows.transitions]]\n",
                "from = \"queued_for_review\"\n",
                "to = \"reviewing\"\n",
                "\n",
                "[[workflows.transitions]]\n",
                "from = \"reviewing\"\n",
                "to = \"reviewed\"\n",
                "\n",
                "[[workflows.transitions]]\n",
                "from = \"reviewed\"\n",
                "to = \"shipping\"\n",
                "\n",
                "[[workflows.transitions]]\n",
                "from = \"shipping\"\n",
                "to = \"shipped\"\n",
            ),
        )
        .expect("simple workflow file should be writable");

        let registry =
            WorkflowRegistry::load(&root).expect("registry should load");
        let simple = registry
            .require("simple")
            .expect("simple workflow should exist");
        assert_eq!(simple.initial_state, "work_item");
        assert_eq!(simple.states.len(), 7);
        assert!(simple.is_terminal_state("shipped"));
        assert!(!simple.is_terminal_state("work_item"));

        assert!(simple
            .validate_transition("work_item", "implementing", false)
            .is_ok());
        assert!(simple
            .validate_transition("implementing", "queued_for_review", false)
            .is_ok());
        assert!(simple
            .validate_transition("reviewing", "reviewed", false)
            .is_ok());
        assert!(simple
            .validate_transition("reviewed", "shipping", false)
            .is_ok());
        assert!(simple
            .validate_transition("shipping", "shipped", false)
            .is_ok());

        // Backward transition should fail without force
        assert!(simple
            .validate_transition("shipped", "work_item", false)
            .is_err());
        // Skipping states should fail without force
        assert!(simple
            .validate_transition("work_item", "shipped", false)
            .is_err());
        // Force allows any transition
        assert!(simple
            .validate_transition("shipped", "work_item", true)
            .is_ok());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn loads_custom_workflow_file_from_toml() {
        let root = repo_root();
        let path = root.join(".knots/workflows.toml");
        std::fs::write(
            &path,
            concat!(
                "[[workflows]]\n",
                "id = \"triage\"\n",
                "description = \"triage flow\"\n",
                "initial_state = \"todo\"\n",
                "states = [\"todo\", \"doing\", \"done\"]\n",
                "terminal_states = [\"done\"]\n",
                "\n",
                "[[workflows.transitions]]\n",
                "from = \"todo\"\n",
                "to = \"doing\"\n",
                "\n",
                "[[workflows.transitions]]\n",
                "from = \"doing\"\n",
                "to = \"done\"\n"
            ),
        )
        .expect("custom workflow file should be writable");

        let registry = WorkflowRegistry::load(&root).expect("workflow registry should load");
        let triage = registry
            .require("triage")
            .expect("triage workflow should exist");
        assert_eq!(triage.initial_state, "todo");
        let _ = std::fs::remove_dir_all(root);
    }
}
