use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionPlanAgent {
    #[serde(default)]
    pub role: String,
    #[serde(default = "default_agent_count")]
    pub count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub specialty: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionPlanKnot {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionPlanStep {
    #[serde(default)]
    pub step_index: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub knot_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionPlanWave {
    #[serde(default)]
    pub wave_index: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub objective: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<ExecutionPlanAgent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub knots: Vec<ExecutionPlanKnot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<ExecutionPlanStep>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionPlanData {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assumptions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub knot_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unassigned_knot_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waves: Vec<ExecutionPlanWave>,
}

impl ExecutionPlanData {
    pub fn is_empty(&self) -> bool {
        self.repo_path.is_none()
            && self.objective.is_none()
            && self.summary.is_none()
            && self.mode.is_none()
            && self.model.is_none()
            && self.assumptions.is_empty()
            && self.knot_ids.is_empty()
            && self.unassigned_knot_ids.is_empty()
            && self.waves.is_empty()
    }

    /// Resolve and normalize every knot ID reference in the plan.
    ///
    /// Accepts bare suffixes (e.g. "a873") and fully-qualified IDs
    /// (e.g. "foolery-a873"). The resolver must return the canonical
    /// fully-qualified ID or an error for unresolvable tokens.
    pub fn normalize_knot_ids<F, E>(&mut self, resolver: F) -> Result<(), E>
    where
        F: Fn(&str) -> Result<String, E>,
    {
        resolve_id_vec(&mut self.knot_ids, &resolver)?;
        resolve_id_vec(&mut self.unassigned_knot_ids, &resolver)?;
        for wave in &mut self.waves {
            for knot in &mut wave.knots {
                knot.id = resolver(&knot.id)?;
            }
            for step in &mut wave.steps {
                resolve_id_vec(&mut step.knot_ids, &resolver)?;
            }
        }
        Ok(())
    }
}

fn resolve_id_vec<F, E>(ids: &mut [String], resolver: &F) -> Result<(), E>
where
    F: Fn(&str) -> Result<String, E>,
{
    for id in ids.iter_mut() {
        *id = resolver(id)?;
    }
    Ok(())
}

const fn default_agent_count() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    use super::{
        ExecutionPlanAgent, ExecutionPlanData, ExecutionPlanKnot, ExecutionPlanStep,
        ExecutionPlanWave,
    };

    #[test]
    fn execution_plan_defaults_to_empty_document() {
        let data = ExecutionPlanData::default();
        assert!(data.is_empty());
    }

    #[test]
    fn execution_plan_round_trips_through_json() {
        let data = ExecutionPlanData {
            repo_path: Some("/repo".to_string()),
            objective: Some("Ship the plan".to_string()),
            summary: Some("Execution plan for caller-selected knots".to_string()),
            mode: Some("autopilot".to_string()),
            model: Some("gpt-5".to_string()),
            assumptions: vec!["assume existing knots are valid".to_string()],
            knot_ids: vec!["knot-1".to_string()],
            unassigned_knot_ids: vec!["knot-2".to_string()],
            waves: vec![ExecutionPlanWave {
                wave_index: 1,
                name: "Persist data".to_string(),
                objective: "Add the typed payload".to_string(),
                agents: vec![ExecutionPlanAgent {
                    role: "backend".to_string(),
                    count: 2,
                    specialty: Some("api".to_string()),
                }],
                knots: vec![ExecutionPlanKnot {
                    id: "knot-1".to_string(),
                    title: "Persist execution-plan data".to_string(),
                }],
                steps: vec![ExecutionPlanStep {
                    step_index: 1,
                    knot_ids: vec!["knot-1".to_string()],
                    notes: Some("Land the schema before API wiring.".to_string()),
                }],
                notes: Some("Wave focuses on persistence only.".to_string()),
            }],
        };
        let json = serde_json::to_string(&data).expect("serialize");
        let parsed: ExecutionPlanData = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, data);
        assert!(!parsed.is_empty());
    }

    #[test]
    fn execution_plan_deserializes_legacy_empty_payload() {
        let parsed: ExecutionPlanData = serde_json::from_str("{}").expect("deserialize");
        assert_eq!(parsed, ExecutionPlanData::default());
    }

    fn mock_resolver(token: &str) -> Result<String, String> {
        match token {
            "a1b2" => Ok("proj-a1b2".to_string()),
            "c3d4" => Ok("proj-c3d4".to_string()),
            "proj-a1b2" => Ok("proj-a1b2".to_string()),
            "proj-c3d4" => Ok("proj-c3d4".to_string()),
            other => Err(format!("unresolvable knot id '{}'", other)),
        }
    }

    #[test]
    fn normalize_resolves_bare_ids_to_qualified() {
        let mut data = ExecutionPlanData {
            knot_ids: vec!["a1b2".to_string()],
            unassigned_knot_ids: vec!["c3d4".to_string()],
            waves: vec![ExecutionPlanWave {
                knots: vec![ExecutionPlanKnot {
                    id: "a1b2".to_string(),
                    title: "task".to_string(),
                }],
                steps: vec![ExecutionPlanStep {
                    step_index: 1,
                    knot_ids: vec!["c3d4".to_string()],
                    notes: None,
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        data.normalize_knot_ids(mock_resolver).unwrap();
        assert_eq!(data.knot_ids, vec!["proj-a1b2"]);
        assert_eq!(data.unassigned_knot_ids, vec!["proj-c3d4"]);
        assert_eq!(data.waves[0].knots[0].id, "proj-a1b2");
        assert_eq!(data.waves[0].steps[0].knot_ids, vec!["proj-c3d4"]);
    }

    #[test]
    fn normalize_passes_through_already_qualified_ids() {
        let mut data = ExecutionPlanData {
            knot_ids: vec!["proj-a1b2".to_string()],
            ..Default::default()
        };
        data.normalize_knot_ids(mock_resolver).unwrap();
        assert_eq!(data.knot_ids, vec!["proj-a1b2"]);
    }

    #[test]
    fn normalize_rejects_unresolvable_ids() {
        let mut data = ExecutionPlanData {
            knot_ids: vec!["unknown".to_string()],
            ..Default::default()
        };
        let err = data.normalize_knot_ids(mock_resolver).unwrap_err();
        assert!(err.contains("unresolvable"));
    }

    #[test]
    fn normalize_noop_on_empty_plan() {
        let mut data = ExecutionPlanData::default();
        data.normalize_knot_ids(mock_resolver).unwrap();
        assert!(data.is_empty());
    }
}
