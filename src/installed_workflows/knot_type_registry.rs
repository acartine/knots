use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::domain::knot_type::KnotType;

use super::normalize_workflow_id;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct WorkflowRef {
    pub workflow_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
}

impl WorkflowRef {
    pub fn new(workflow_id: impl Into<String>, version: Option<u32>) -> Self {
        Self {
            workflow_id: normalize_workflow_id(&workflow_id.into()),
            version,
        }
    }

    pub fn normalize(mut self) -> Self {
        self.workflow_id = normalize_workflow_id(&self.workflow_id);
        self
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct KnotTypeWorkflowConfig {
    pub default: WorkflowRef,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub registered: Vec<WorkflowRef>,
}

impl KnotTypeWorkflowConfig {
    pub fn normalize(mut self) -> Self {
        self.default = self.default.normalize();
        let mut registered = Vec::new();
        push_unique(&mut registered, self.default.clone());
        for workflow in self.registered.drain(..) {
            push_unique(&mut registered, workflow.normalize());
        }
        self.registered = registered;
        self
    }

    pub fn register(&mut self, workflow: WorkflowRef) {
        push_unique(&mut self.registered, workflow);
    }
}

pub fn normalize_knot_type_workflows(
    mut raw: BTreeMap<String, KnotTypeWorkflowConfig>,
) -> BTreeMap<String, KnotTypeWorkflowConfig> {
    let mut normalized = BTreeMap::new();
    for (knot_type, config) in raw.iter_mut() {
        let Ok(parsed) = knot_type.parse::<KnotType>() else {
            continue;
        };
        normalized.insert(
            parsed.as_str().to_string(),
            std::mem::take(config).normalize(),
        );
    }
    normalized
}

fn push_unique(items: &mut Vec<WorkflowRef>, workflow: WorkflowRef) {
    if !items.iter().any(|item| item == &workflow) {
        items.push(workflow);
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_knot_type_workflows, KnotTypeWorkflowConfig, WorkflowRef};
    use std::collections::BTreeMap;

    #[test]
    fn normalize_deduplicates_default_and_registered() {
        let config = KnotTypeWorkflowConfig {
            default: WorkflowRef::new("work_sdlc", Some(1)),
            registered: vec![
                WorkflowRef::new("work_sdlc", Some(1)),
                WorkflowRef::new("custom_flow", None),
            ],
        }
        .normalize();

        assert_eq!(config.registered.len(), 2);
        assert_eq!(config.registered[0], WorkflowRef::new("work_sdlc", Some(1)));
    }

    #[test]
    fn normalize_knot_type_map_discards_unknown_keys() {
        let normalized = normalize_knot_type_workflows(BTreeMap::from([
            (
                "work".to_string(),
                KnotTypeWorkflowConfig {
                    default: WorkflowRef::new("work_sdlc", None),
                    registered: Vec::new(),
                },
            ),
            (
                "unknown".to_string(),
                KnotTypeWorkflowConfig {
                    default: WorkflowRef::new("bad", None),
                    registered: Vec::new(),
                },
            ),
        ]));

        assert!(normalized.contains_key("work"));
        assert!(!normalized.contains_key("unknown"));
    }
}
