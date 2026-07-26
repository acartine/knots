use serde::{Deserialize, Serialize};

mod io;
mod lease_ops;
mod plan_ops;

pub use io::enqueue_and_wait_with_context;
#[allow(unused_imports)]
pub use io::QueueError;
pub use lease_ops::{LeaseCreateOperation, LeaseExtendOperation, LeaseTerminateOperation};
pub use plan_ops::{
    PlanStepAddOperation, PlanStepMoveOperation, PlanStepRemoveOperation, PlanWaveAddOperation,
    PlanWaveMoveOperation, PlanWaveRemoveOperation,
};

#[cfg(test)]
use io::{
    drain_pending_requests, enqueue_and_wait, enqueue_request, list_request_files,
    read_response_file, QueuePaths,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewOperation {
    pub title: String,
    pub description: Option<String>,
    pub acceptance: Option<String>,
    #[serde(default)]
    pub verification_steps: Vec<String>,
    pub state: Option<String>,
    pub profile: Option<String>,
    pub workflow: Option<String>,
    pub fast: bool,
    pub exploration: bool,
    pub knot_type: Option<String>,
    pub objective: Option<String>,
    pub gate_owner_kind: Option<String>,
    pub gate_failure_modes: Vec<String>,
    pub tags: Vec<String>,
    pub scope: crate::cli_scope::ScopeArgs,
    pub lease_id: Option<String>,
    #[serde(default)]
    pub json: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuickNewOperation {
    pub title: String,
    pub description: Option<String>,
    pub state: Option<String>,
    #[serde(default)]
    pub json: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateOperation {
    pub id: String,
    pub state: String,
    pub force: bool,
    pub approve_terminal_cascade: bool,
    pub if_match: Option<String>,
    pub actor_kind: Option<String>,
    pub agent_name: Option<String>,
    pub agent_model: Option<String>,
    pub agent_version: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateOperation {
    pub id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub acceptance: Option<String>,
    pub priority: Option<i64>,
    pub status: Option<String>,
    pub knot_type: Option<String>,
    pub add_tags: Vec<String>,
    pub remove_tags: Vec<String>,
    pub add_invariants: Vec<String>,
    pub remove_invariants: Vec<String>,
    pub clear_invariants: bool,
    #[serde(default)]
    pub add_verification_steps: Vec<String>,
    #[serde(default)]
    pub remove_verification_steps: Vec<String>,
    #[serde(default)]
    pub clear_verification_steps: bool,
    pub gate_owner_kind: Option<String>,
    pub gate_failure_modes: Vec<String>,
    pub clear_gate_failure_modes: bool,
    pub scope: crate::cli_scope::ScopeArgs,
    pub execution_plan_file: Option<String>,
    pub objective: Option<String>,
    pub add_note: Option<String>,
    pub note_username: Option<String>,
    pub note_datetime: Option<String>,
    pub note_agentname: Option<String>,
    pub note_model: Option<String>,
    pub note_version: Option<String>,
    pub add_handoff_capsule: Option<String>,
    pub handoff_username: Option<String>,
    pub handoff_datetime: Option<String>,
    pub handoff_agentname: Option<String>,
    pub handoff_model: Option<String>,
    pub handoff_version: Option<String>,
    pub if_match: Option<String>,
    pub actor_kind: Option<String>,
    pub agent_name: Option<String>,
    pub agent_model: Option<String>,
    pub agent_version: Option<String>,
    pub force: bool,
    pub approve_terminal_cascade: bool,
    pub lease_id: Option<String>,
    #[serde(default)]
    pub json: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NextOperation {
    pub id: String,
    pub expected_state: Option<String>,
    pub json: bool,
    pub approve_terminal_cascade: bool,
    pub actor_kind: Option<String>,
    pub agent_name: Option<String>,
    pub agent_model: Option<String>,
    pub agent_version: Option<String>,
    pub lease_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RollbackOperation {
    pub id: String,
    pub dry_run: bool,
    pub actor_kind: Option<String>,
    pub agent_name: Option<String>,
    pub agent_model: Option<String>,
    pub agent_version: Option<String>,
    #[serde(default)]
    pub json: bool,
    #[serde(default)]
    pub outcome: Option<String>,
    #[serde(default)]
    pub lease_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClaimOperation {
    pub id: String,
    pub json: bool,
    pub verbose: bool,
    pub agent_name: Option<String>,
    pub agent_model: Option<String>,
    pub agent_version: Option<String>,
    pub lease_id: Option<String>,
    pub timeout_seconds: Option<u64>,
    #[serde(default)]
    pub e2e: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PollClaimOperation {
    pub stage: Option<String>,
    pub owner: Option<String>,
    pub json: bool,
    pub agent_name: Option<String>,
    pub agent_model: Option<String>,
    pub agent_version: Option<String>,
    pub timeout_seconds: Option<u64>,
    #[serde(default)]
    pub e2e: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GateEvaluateOperation {
    pub id: String,
    pub decision: String,
    pub invariant: Option<String>,
    pub json: bool,
    pub actor_kind: Option<String>,
    pub agent_name: Option<String>,
    pub agent_model: Option<String>,
    pub agent_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EdgeOperation {
    pub src: String,
    pub kind: String,
    pub dst: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepAnnotateOperation {
    pub id: String,
    pub actor_kind: Option<String>,
    pub agent_name: Option<String>,
    pub agent_model: Option<String>,
    pub agent_version: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum WriteOperation {
    New(NewOperation),
    QuickNew(QuickNewOperation),
    State(StateOperation),
    Update(UpdateOperation),
    Next(NextOperation),
    Rollback(RollbackOperation),
    Claim(ClaimOperation),
    PollClaim(PollClaimOperation),
    GateEvaluate(GateEvaluateOperation),
    PlanWaveAdd(PlanWaveAddOperation),
    PlanWaveRemove(PlanWaveRemoveOperation),
    PlanWaveMove(PlanWaveMoveOperation),
    PlanStepAdd(PlanStepAddOperation),
    PlanStepRemove(PlanStepRemoveOperation),
    PlanStepMove(PlanStepMoveOperation),
    EdgeAdd(EdgeOperation),
    EdgeRemove(EdgeOperation),
    StepAnnotate(StepAnnotateOperation),
    LeaseCreate(LeaseCreateOperation),
    LeaseTerminate(LeaseTerminateOperation),
    LeaseExtend(LeaseExtendOperation),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueuedWriteRequest {
    pub request_id: String,
    pub repo_root: String,
    pub store_root: String,
    pub distribution: String,
    pub project_id: Option<String>,
    pub db_path: String,
    pub response_path: String,
    pub operation: WriteOperation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueuedWriteResponse {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

impl QueuedWriteResponse {
    pub fn success(output: String) -> Self {
        Self {
            success: true,
            output,
            error: None,
        }
    }

    pub fn failure(error: String) -> Self {
        Self {
            success: false,
            output: String::new(),
            error: Some(error),
        }
    }
}

#[cfg(test)]
mod tests;
