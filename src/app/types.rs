use serde::Serialize;

use crate::db::{EdgeRecord, KnotCacheRecord};
use crate::domain::gate::GateData;
use crate::domain::invariant::Invariant;
use crate::domain::knot_type::{parse_knot_type, KnotType};
use crate::domain::lease::LeaseData;
use crate::domain::metadata::MetadataEntry;
use crate::domain::step_history::StepRecord;

use super::helpers::canonical_profile_id;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct KnotView {
    pub id: String,
    pub alias: Option<String>,
    pub title: String,
    pub state: String,
    pub updated_at: String,
    pub body: Option<String>,
    pub description: Option<String>,
    pub acceptance: Option<String>,
    pub priority: Option<i64>,
    #[serde(rename = "type")]
    pub knot_type: KnotType,
    pub tags: Vec<String>,
    pub notes: Vec<MetadataEntry>,
    pub handoff_capsules: Vec<MetadataEntry>,
    pub invariants: Vec<Invariant>,
    pub step_history: Vec<StepRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<GateData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease: Option<LeaseData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_id: Option<String>,
    pub workflow_id: String,
    pub profile_id: String,
    pub profile_etag: Option<String>,
    pub deferred_from_state: Option<String>,
    pub blocked_from_state: Option<String>,
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<EdgeView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub child_summaries: Vec<ChildSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EdgeView {
    pub src: String,
    pub kind: String,
    pub dst: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ChildSummary {
    pub id: String,
    pub title: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GateEvaluationResult {
    pub gate: KnotView,
    pub decision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invariant: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reopened: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateDecision {
    Yes,
    No,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ColdKnotView {
    pub id: String,
    pub title: String,
    pub state: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PullDriftWarning {
    pub unpushed_event_files: u64,
    pub threshold: u64,
}

#[derive(Debug, Clone, Default)]
pub struct StateActorMetadata {
    pub actor_kind: Option<String>,
    pub agent_name: Option<String>,
    pub agent_model: Option<String>,
    pub agent_version: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateKnotPatch {
    pub title: Option<String>,
    pub description: Option<String>,
    pub acceptance: Option<String>,
    pub priority: Option<i64>,
    pub status: Option<String>,
    pub knot_type: Option<KnotType>,
    pub add_tags: Vec<String>,
    pub remove_tags: Vec<String>,
    pub add_invariants: Vec<Invariant>,
    pub remove_invariants: Vec<Invariant>,
    pub clear_invariants: bool,
    pub gate_owner_kind: Option<crate::domain::gate::GateOwnerKind>,
    pub gate_failure_modes: Option<std::collections::BTreeMap<String, Vec<String>>>,
    pub clear_gate_failure_modes: bool,
    pub add_note: Option<crate::domain::metadata::MetadataEntryInput>,
    pub add_handoff_capsule: Option<crate::domain::metadata::MetadataEntryInput>,
    pub expected_profile_etag: Option<String>,
    pub force: bool,
    pub state_actor: StateActorMetadata,
}

impl UpdateKnotPatch {
    pub(crate) fn has_changes(&self) -> bool {
        self.title.is_some()
            || self.description.is_some()
            || self.acceptance.is_some()
            || self.priority.is_some()
            || self.status.is_some()
            || self.knot_type.is_some()
            || !self.add_tags.is_empty()
            || !self.remove_tags.is_empty()
            || !self.add_invariants.is_empty()
            || !self.remove_invariants.is_empty()
            || self.clear_invariants
            || self.gate_owner_kind.is_some()
            || self.gate_failure_modes.is_some()
            || self.clear_gate_failure_modes
            || self.add_note.is_some()
            || self.add_handoff_capsule.is_some()
    }
}

#[derive(Debug, Clone, Default)]
pub struct CreateKnotOptions {
    pub knot_type: KnotType,
    pub gate_data: GateData,
    pub lease_data: LeaseData,
    pub acceptance: Option<String>,
}

impl From<KnotCacheRecord> for KnotView {
    fn from(value: KnotCacheRecord) -> Self {
        let profile_id = canonical_profile_id(&value.profile_id, &value.workflow_id);
        let knot_type = parse_knot_type(value.knot_type.as_deref());
        let gate = (knot_type == KnotType::Gate).then_some(value.gate_data.clone());
        let lease = (knot_type == KnotType::Lease).then_some(value.lease_data.clone());
        Self {
            id: value.id,
            alias: None,
            title: value.title,
            state: value.state,
            updated_at: value.updated_at,
            body: value.body,
            description: value.description,
            acceptance: value.acceptance,
            priority: value.priority,
            knot_type,
            tags: value.tags,
            notes: value.notes,
            handoff_capsules: value.handoff_capsules,
            invariants: value.invariants,
            step_history: value.step_history,
            gate,
            lease,
            lease_id: value.lease_id,
            workflow_id: value.workflow_id,
            profile_id,
            profile_etag: value.profile_etag,
            deferred_from_state: value.deferred_from_state,
            blocked_from_state: value.blocked_from_state,
            created_at: value.created_at,
            edges: Vec::new(),
            child_summaries: Vec::new(),
        }
    }
}

impl From<EdgeRecord> for EdgeView {
    fn from(value: EdgeRecord) -> Self {
        Self {
            src: value.src,
            kind: value.kind,
            dst: value.dst,
        }
    }
}
