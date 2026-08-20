use std::time::Duration;

use serde_json::json;

use crate::db::{self, UpsertKnotHot};
use crate::domain::knot_type::parse_knot_type;
use crate::events::{EventRecord, FullEvent, FullEventKind, IndexEvent, IndexEventKind};
use crate::locks::FileLock;
use crate::workflow_runtime;

use super::error::AppError;
use super::helpers::{build_knot_head_data, resolve_step_metadata, KnotHeadData};
use super::App;

impl App {
    /// Set this knot's (normally a lease knot's) `lease_expiry_ts` and push
    /// a fresh `idx.knot_head` event carrying the new value, so a future
    /// `kno push` replicates it. Every expiry change funnels through here
    /// -- creation, claim/heartbeat renewal, explicit extend, and
    /// lazy-recovery re-arm -- so this is the single point where expiry
    /// starts leaving the machine that set it (see knot `e045`).
    ///
    /// The event's `updated_at` payload field is pinned to the record's
    /// existing `updated_at`, not "now": a heartbeat is not a content
    /// change, and knots must not reorder in `kno ls` just because their
    /// lease was renewed.
    pub fn set_lease_expiry(&self, id: &str, ts: i64) -> Result<(), AppError> {
        let id = self.resolve_knot_token(id)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        self.set_lease_expiry_locked(&id, ts)
    }

    /// Same as `set_lease_expiry`, but for callers that already hold the
    /// repo and cache locks (e.g. `release_lease_knot_locked`, which is
    /// itself always called from inside an outer locked scope). Acquiring
    /// the locks again here would deadlock against the caller's own guard.
    pub(super) fn set_lease_expiry_locked(&self, id: &str, ts: i64) -> Result<(), AppError> {
        let record =
            db::get_knot_hot(&self.conn, id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
        let profile = self.resolve_profile_for_record(&record)?;
        let knot_type = parse_knot_type(record.knot_type.as_deref());
        let terminal = workflow_runtime::is_terminal_state(
            &self.profile_registry,
            profile.id.as_str(),
            knot_type,
            &record.state,
        )?;
        let (step_metadata, next_step_metadata) = resolve_step_metadata(
            &self.profile_registry,
            profile.workflow_id.as_str(),
            profile.id.as_str(),
            knot_type,
            &record.gate_data,
            &record.state,
        )?;
        let idx_event = IndexEvent::new(
            IndexEventKind::KnotHead,
            build_knot_head_data(KnotHeadData {
                knot_id: &record.id,
                title: &record.title,
                state: &record.state,
                workflow_id: &profile.workflow_id,
                profile_id: profile.id.as_str(),
                updated_at: &record.updated_at,
                terminal,
                deferred_from_state: record.deferred_from_state.as_deref(),
                blocked_from_state: record.blocked_from_state.as_deref(),
                invariants: &record.invariants,
                knot_type,
                gate_data: &record.gate_data,
                execution_plan_data: &record.execution_plan_data,
                scope_data: Some(&record.scope_data),
                step_metadata: step_metadata.as_ref(),
                next_step_metadata: next_step_metadata.as_ref(),
                lease_expiry_ts: ts,
            }),
        );
        self.writer.write(&EventRecord::index(idx_event))?;
        crate::db::update_lease_expiry_ts(&self.conn, id, ts)
            .map_err(|e| AppError::Io(std::io::Error::other(e.to_string())))
    }

    pub fn set_lease_id(&self, knot_id: &str, lease_id: Option<&str>) -> Result<(), AppError> {
        let id = self.resolve_knot_token(knot_id)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        let record =
            db::get_knot_hot(&self.conn, &id)?.ok_or_else(|| AppError::NotFound(id.clone()))?;
        let event = FullEvent::new(
            id.clone(),
            FullEventKind::KnotLeaseIdSet,
            json!({ "lease_id": lease_id }),
        );
        self.writer.write(&EventRecord::full(event))?;
        db::upsert_knot_hot(
            &self.conn,
            &UpsertKnotHot {
                id: &record.id,
                title: &record.title,
                state: &record.state,
                updated_at: &record.updated_at,
                body: record.body.as_deref(),
                description: record.description.as_deref(),
                acceptance: record.acceptance.as_deref(),
                priority: record.priority,
                knot_type: record.knot_type.as_deref(),
                tags: &record.tags,
                notes: &record.notes,
                handoff_capsules: &record.handoff_capsules,
                invariants: &record.invariants,
                verification_steps: &record.verification_steps,
                step_history: &record.step_history,
                gate_data: &record.gate_data,
                lease_data: &record.lease_data,
                execution_plan_data: &record.execution_plan_data,
                lease_id,
                lease_expiry_ts: record.lease_expiry_ts,
                workflow_id: &record.workflow_id,
                profile_id: &record.profile_id,
                profile_etag: record.profile_etag.as_deref(),
                deferred_from_state: record.deferred_from_state.as_deref(),
                blocked_from_state: record.blocked_from_state.as_deref(),
                created_at: record.created_at.as_deref(),
            },
        )?;
        Ok(())
    }
}
