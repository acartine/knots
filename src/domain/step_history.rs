use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepRecord {
    pub id: String,
    pub step: String,
    pub phase: String,
    pub from_state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_state: Option<String>,
    pub status: StepStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Started,
    Completed,
    Aborted,
    Failed,
}

impl StepRecord {
    pub fn new_started(
        step: &str,
        phase: &str,
        from_state: &str,
        started_at: &str,
        actor: &StepActorInfo,
    ) -> Self {
        Self {
            id: new_step_id(),
            step: step.to_string(),
            phase: phase.to_string(),
            from_state: from_state.to_string(),
            to_state: None,
            status: StepStatus::Started,
            actor_kind: actor.actor_kind.clone(),
            agent_id: actor.agent_id.clone(),
            agent_name: actor.agent_name.clone(),
            agent_model: actor.agent_model.clone(),
            agent_version: actor.agent_version.clone(),
            agent_command: actor.agent_command.clone(),
            session_id: actor.session_id.clone(),
            started_at: started_at.to_string(),
            ended_at: None,
            metadata: None,
        }
    }

    pub fn is_active(&self) -> bool {
        self.status == StepStatus::Started
    }
}

#[derive(Debug, Clone, Default)]
pub struct StepActorInfo {
    pub actor_kind: Option<String>,
    pub agent_id: Option<String>,
    pub agent_name: Option<String>,
    pub agent_model: Option<String>,
    pub agent_version: Option<String>,
    pub agent_command: Option<String>,
    pub session_id: Option<String>,
}

pub fn derive_phase(state: &str) -> &str {
    if state.ends_with("_review") {
        "review"
    } else {
        "action"
    }
}

fn new_step_id() -> String {
    Uuid::now_v7().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_started_record_has_correct_defaults() {
        let actor = StepActorInfo {
            actor_kind: Some("agent".to_string()),
            agent_name: Some("claude".to_string()),
            agent_model: Some("opus".to_string()),
            agent_version: Some("4.6".to_string()),
            ..Default::default()
        };
        let record = StepRecord::new_started(
            "implementation",
            "action",
            "ready_for_implementation",
            "2026-03-06T10:00:00Z",
            &actor,
        );
        assert!(record.is_active());
        assert_eq!(record.status, StepStatus::Started);
        assert_eq!(record.step, "implementation");
        assert_eq!(record.phase, "action");
        assert_eq!(record.from_state, "ready_for_implementation");
        assert!(record.to_state.is_none());
        assert!(record.ended_at.is_none());
        assert_eq!(record.agent_name.as_deref(), Some("claude"));
        assert!(!record.id.is_empty());
    }

    #[test]
    fn derive_phase_action_vs_review() {
        assert_eq!(derive_phase("implementation"), "action");
        assert_eq!(derive_phase("planning"), "action");
        assert_eq!(derive_phase("implementation_review"), "review");
        assert_eq!(derive_phase("plan_review"), "review");
        assert_eq!(derive_phase("shipment_review"), "review");
    }

    #[test]
    fn step_record_serialization_roundtrip() {
        let actor = StepActorInfo {
            actor_kind: Some("agent".to_string()),
            agent_name: Some("test-agent".to_string()),
            ..Default::default()
        };
        let record = StepRecord::new_started(
            "planning",
            "action",
            "ready_for_planning",
            "2026-03-06T10:00:00Z",
            &actor,
        );
        let json = serde_json::to_string(&record).unwrap();
        let parsed: StepRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, record);
    }

    #[test]
    fn step_status_serializes_as_snake_case() {
        let json = serde_json::to_string(&StepStatus::Started).unwrap();
        assert_eq!(json, "\"started\"");
        let json = serde_json::to_string(&StepStatus::Completed).unwrap();
        assert_eq!(json, "\"completed\"");
    }

    #[test]
    fn optional_fields_omitted_when_none() {
        let actor = StepActorInfo::default();
        let record = StepRecord::new_started(
            "planning",
            "action",
            "ready_for_planning",
            "2026-03-06T10:00:00Z",
            &actor,
        );
        let json = serde_json::to_string(&record).unwrap();
        assert!(!json.contains("to_state"));
        assert!(!json.contains("ended_at"));
        assert!(!json.contains("metadata"));
    }
}
