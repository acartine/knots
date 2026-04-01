use std::time::Duration;

use serde_json::json;

use crate::db::{self, UpsertKnotHot};
use crate::domain::knot_type::parse_knot_type;
use crate::events::{
    new_event_id, now_utc_rfc3339, EventRecord, FullEvent, FullEventKind, IndexEvent,
    IndexEventKind,
};
use crate::locks::FileLock;
use crate::workflow_runtime;

use super::error::AppError;
use super::helpers::{build_knot_head_data, parse_edge_direction, KnotHeadData};
use super::types::{EdgeView, StateActorMetadata};
use super::App;

impl App {
    pub fn add_edge(&self, src: &str, kind: &str, dst: &str) -> Result<EdgeView, AppError> {
        let src = self.resolve_knot_token(src)?;
        let dst = self.resolve_knot_token(dst)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        self.apply_edge_change(&src, kind, &dst, true)
    }

    pub fn remove_edge(&self, src: &str, kind: &str, dst: &str) -> Result<EdgeView, AppError> {
        let src = self.resolve_knot_token(src)?;
        let dst = self.resolve_knot_token(dst)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        self.apply_edge_change(&src, kind, &dst, false)
    }

    pub fn list_edges(&self, id: &str, direction: &str) -> Result<Vec<EdgeView>, AppError> {
        let id = self.resolve_knot_token(id)?;
        let direction = parse_edge_direction(direction)?;
        let rows = db::list_edges(&self.conn, &id, direction)?;
        Ok(rows.into_iter().map(EdgeView::from).collect())
    }

    pub fn list_layout_edges(&self) -> Result<Vec<EdgeView>, AppError> {
        let mut rows = db::list_edges_by_kind(&self.conn, "parent_of")?;
        rows.extend(db::list_edges_by_kind(&self.conn, "blocked_by")?);
        rows.extend(db::list_edges_by_kind(&self.conn, "blocks")?);
        Ok(rows.into_iter().map(EdgeView::from).collect())
    }

    fn apply_edge_change(
        &self,
        src: &str,
        kind: &str,
        dst: &str,
        add: bool,
    ) -> Result<EdgeView, AppError> {
        if src.trim().is_empty() || kind.trim().is_empty() || dst.trim().is_empty() {
            return Err(AppError::InvalidArgument(
                "src, kind, and dst are required".to_string(),
            ));
        }
        let current = db::get_knot_hot(&self.conn, src)?
            .ok_or_else(|| AppError::NotFound(src.to_string()))?;
        let occurred_at = now_utc_rfc3339();
        let full_kind = if add {
            FullEventKind::KnotEdgeAdd
        } else {
            FullEventKind::KnotEdgeRemove
        };
        let full_event = FullEvent::with_identity(
            new_event_id(),
            occurred_at.clone(),
            src.to_string(),
            full_kind.as_str(),
            json!({"kind": kind, "dst": dst}),
        );
        self.writer.write(&EventRecord::full(full_event))?;
        let profile = self.resolve_profile_for_record(&current)?;
        let profile_id = profile.id.clone();
        let index_event_id = new_event_id();
        self.write_edge_index_event(
            &index_event_id,
            src,
            &current,
            profile,
            &profile_id,
            &occurred_at,
        )?;
        if add {
            db::insert_edge(&self.conn, src, kind, dst)?;
        } else {
            db::delete_edge(&self.conn, src, kind, dst)?;
        }
        self.persist_edge_knot_hot(
            src,
            &current,
            &profile.workflow_id,
            &profile_id,
            &occurred_at,
            &index_event_id,
        )?;
        if !add && kind == "blocked_by" {
            self.resume_blocked_dependents_locked(src, &StateActorMetadata::default())?;
        }
        Ok(EdgeView {
            src: src.to_string(),
            kind: kind.to_string(),
            dst: dst.to_string(),
        })
    }

    fn write_edge_index_event(
        &self,
        event_id: &str,
        src: &str,
        current: &db::KnotCacheRecord,
        profile: &crate::workflow::ProfileDefinition,
        profile_id: &str,
        occurred_at: &str,
    ) -> Result<(), AppError> {
        let knot_type = parse_knot_type(current.knot_type.as_deref());
        let terminal = workflow_runtime::is_terminal_state(
            &self.profile_registry,
            profile_id,
            knot_type,
            &current.state,
        )?;
        let idx_event = IndexEvent::with_identity(
            event_id.to_string(),
            occurred_at.to_string(),
            IndexEventKind::KnotHead.as_str(),
            build_knot_head_data(KnotHeadData {
                knot_id: src,
                title: &current.title,
                state: &current.state,
                workflow_id: &profile.workflow_id,
                profile_id,
                updated_at: occurred_at,
                terminal,
                deferred_from_state: current.deferred_from_state.as_deref(),
                blocked_from_state: current.blocked_from_state.as_deref(),
                invariants: &current.invariants,
                knot_type,
                gate_data: &current.gate_data,
                step_metadata: None,
                next_step_metadata: None,
            }),
        );
        self.writer.write(&EventRecord::index(idx_event))?;
        Ok(())
    }

    fn persist_edge_knot_hot(
        &self,
        src: &str,
        current: &db::KnotCacheRecord,
        workflow_id: &str,
        profile_id: &str,
        occurred_at: &str,
        profile_etag: &str,
    ) -> Result<(), AppError> {
        db::upsert_knot_hot(
            &self.conn,
            &UpsertKnotHot {
                id: src,
                title: &current.title,
                state: &current.state,
                updated_at: occurred_at,
                body: current.body.as_deref(),
                description: current.description.as_deref(),
                acceptance: current.acceptance.as_deref(),
                priority: current.priority,
                knot_type: current.knot_type.as_deref(),
                tags: &current.tags,
                notes: &current.notes,
                handoff_capsules: &current.handoff_capsules,
                invariants: &current.invariants,
                step_history: &current.step_history,
                gate_data: &current.gate_data,
                lease_data: &current.lease_data,
                lease_id: current.lease_id.as_deref(),
                workflow_id,
                profile_id,
                profile_etag: Some(profile_etag),
                deferred_from_state: current.deferred_from_state.as_deref(),
                blocked_from_state: current.blocked_from_state.as_deref(),
                created_at: current.created_at.as_deref(),
            },
        )?;
        Ok(())
    }
}
