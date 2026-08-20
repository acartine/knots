//! `MetadataProjection`: a mutable, in-memory copy of a `knot_hot` row used
//! by the sync applier to fold one event's changes onto the existing cache
//! record before writing it back. Split out of `apply_helpers.rs` to keep
//! that file under the repo's 500-line cap.

use rusqlite::Connection;

use crate::db::{self, KnotCacheRecord, UpsertKnotHot};
use crate::domain::execution_plan::ExecutionPlanData;
use crate::domain::gate::GateData;
use crate::domain::invariant::Invariant;
use crate::domain::lease::LeaseData;
use crate::domain::metadata::MetadataEntry;
use crate::domain::scope::ScopeData;
use crate::domain::step_history::StepRecord;

use super::SyncError;

pub(super) struct MetadataProjection {
    pub title: String,
    pub state: String,
    pub updated_at: String,
    pub body: Option<String>,
    pub description: Option<String>,
    pub acceptance: Option<String>,
    pub priority: Option<i64>,
    pub knot_type: Option<String>,
    pub tags: Vec<String>,
    pub notes: Vec<MetadataEntry>,
    pub handoff_capsules: Vec<MetadataEntry>,
    pub invariants: Vec<Invariant>,
    pub verification_steps: Vec<String>,
    pub step_history: Vec<StepRecord>,
    pub gate_data: GateData,
    pub lease_data: LeaseData,
    pub execution_plan_data: ExecutionPlanData,
    pub scope_data: ScopeData,
    pub lease_id: Option<String>,
    pub lease_expiry_ts: i64,
    pub workflow_id: String,
    pub profile_id: String,
    pub profile_etag: Option<String>,
    pub deferred_from_state: Option<String>,
    pub blocked_from_state: Option<String>,
    pub created_at: Option<String>,
}

impl MetadataProjection {
    pub fn from_existing(existing: &KnotCacheRecord) -> Self {
        Self {
            title: existing.title.clone(),
            state: existing.state.clone(),
            updated_at: existing.updated_at.clone(),
            body: existing.body.clone(),
            description: existing.description.clone(),
            acceptance: existing.acceptance.clone(),
            priority: existing.priority,
            knot_type: existing.knot_type.clone(),
            tags: existing.tags.clone(),
            notes: existing.notes.clone(),
            handoff_capsules: existing.handoff_capsules.clone(),
            invariants: existing.invariants.clone(),
            verification_steps: existing.verification_steps.clone(),
            step_history: existing.step_history.clone(),
            gate_data: existing.gate_data.clone(),
            lease_data: existing.lease_data.clone(),
            execution_plan_data: existing.execution_plan_data.clone(),
            scope_data: existing.scope_data.clone(),
            lease_id: existing.lease_id.clone(),
            lease_expiry_ts: existing.lease_expiry_ts,
            workflow_id: existing.workflow_id.clone(),
            profile_id: existing.profile_id.clone(),
            profile_etag: existing.profile_etag.clone(),
            deferred_from_state: existing.deferred_from_state.clone(),
            blocked_from_state: existing.blocked_from_state.clone(),
            created_at: existing.created_at.clone(),
        }
    }

    pub fn upsert(&self, conn: &Connection, id: &str) -> Result<(), SyncError> {
        db::upsert_knot_hot(
            conn,
            &UpsertKnotHot {
                id,
                title: &self.title,
                state: &self.state,
                updated_at: &self.updated_at,
                body: self.body.as_deref(),
                description: self.description.as_deref(),
                acceptance: self.acceptance.as_deref(),
                priority: self.priority,
                knot_type: self.knot_type.as_deref(),
                tags: &self.tags,
                notes: &self.notes,
                handoff_capsules: &self.handoff_capsules,
                invariants: &self.invariants,
                verification_steps: &self.verification_steps,
                step_history: &self.step_history,
                gate_data: &self.gate_data,
                lease_data: &self.lease_data,
                execution_plan_data: &self.execution_plan_data,
                lease_id: self.lease_id.as_deref(),
                lease_expiry_ts: self.lease_expiry_ts,
                workflow_id: &self.workflow_id,
                profile_id: &self.profile_id,
                profile_etag: self.profile_etag.as_deref(),
                deferred_from_state: self.deferred_from_state.as_deref(),
                blocked_from_state: self.blocked_from_state.as_deref(),
                created_at: self.created_at.as_deref(),
            },
        )?;
        db::update_knot_scope_data(conn, id, &self.scope_data)?;
        Ok(())
    }
}
