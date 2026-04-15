use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanWaveAddOperation {
    pub id: String,
    pub name: String,
    pub objective: String,
    pub at: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanWaveRemoveOperation {
    pub id: String,
    pub wave: u32,
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanWaveMoveOperation {
    pub id: String,
    pub from_index: u32,
    pub to_index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanStepAddOperation {
    pub id: String,
    pub wave: u32,
    pub knot_ids: Vec<String>,
    pub notes: Option<String>,
    pub at: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanStepRemoveOperation {
    pub id: String,
    pub wave: u32,
    pub step: u32,
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanStepMoveOperation {
    pub id: String,
    pub wave: u32,
    pub from_index: u32,
    pub to_index: u32,
}
