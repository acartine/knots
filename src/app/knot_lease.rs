use std::time::Duration;

use serde_json::json;

use crate::db::{self, UpsertKnotHot};
use crate::events::{EventRecord, FullEvent, FullEventKind};
use crate::locks::FileLock;

use super::error::AppError;
use super::App;

impl App {
    pub fn set_lease_expiry(&self, id: &str, ts: i64) -> Result<(), AppError> {
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
                step_history: &record.step_history,
                gate_data: &record.gate_data,
                lease_data: &record.lease_data,
                lease_id,
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
