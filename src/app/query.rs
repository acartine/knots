use std::time::Duration;

use crate::db::{self, UpsertKnotHot};
use crate::domain::knot_type::parse_knot_type;
use crate::domain::step_history::StepActorInfo;
use crate::events::{new_event_id, now_utc_rfc3339, EventRecord, IndexEvent, IndexEventKind};
use crate::locks::FileLock;
use crate::workflow_runtime;

use super::error::AppError;
use super::helpers::{
    annotate_step_history, build_knot_head_data, resolve_step_metadata, KnotHeadData,
};
use super::rehydrate::rehydrate_from_events;
use super::types::{ChildSummary, ColdKnotView, EdgeView, KnotView, LeaseAgentView};
use super::App;

impl App {
    pub fn list_knots(&self) -> Result<Vec<KnotView>, AppError> {
        self.maybe_auto_sync_for_read()?;
        let mut knots: Vec<KnotView> =
            crate::trace::measure("list_knot_hot", || db::list_knot_hot(&self.conn))?
                .into_iter()
                .map(KnotView::from)
                .collect();
        for knot in &mut knots {
            workflow_runtime::enrich_step_metadata(knot, &self.profile_registry);
        }
        self.apply_aliases_to_knots(knots)
    }

    pub fn show_knot(&self, id: &str) -> Result<Option<KnotView>, AppError> {
        let id = self.resolve_knot_token(id)?;
        self.maybe_auto_sync_for_read()?;
        if let Some(knot) =
            crate::trace::measure("get_knot_hot", || db::get_knot_hot(&self.conn, &id))?
        {
            let mut view = self.apply_alias_to_knot(KnotView::from(knot))?;
            let edges = crate::trace::measure("list_edges", || {
                db::list_edges(&self.conn, &id, db::EdgeDirection::Both)
            })?;
            view.edges = edges.into_iter().map(EdgeView::from).collect();
            view.child_summaries = view
                .edges
                .iter()
                .filter(|e| e.kind == "parent_of" && e.src == id)
                .filter_map(|e| {
                    db::get_knot_hot(&self.conn, &e.dst)
                        .ok()
                        .flatten()
                        .map(|child| ChildSummary {
                            id: child.id,
                            title: child.title,
                            state: child.state,
                        })
                })
                .collect();
            workflow_runtime::enrich_step_metadata(&mut view, &self.profile_registry);
            self.resolve_lease_agent(&mut view);
            return Ok(Some(view));
        }
        Ok(None)
    }

    pub fn step_annotate(&self, id: &str, actor: &StepActorInfo) -> Result<KnotView, AppError> {
        let id = self.resolve_knot_token(id)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        let current =
            db::get_knot_hot(&self.conn, &id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
        if !current.step_history.iter().any(|r| r.is_active()) {
            return Err(AppError::InvalidArgument(
                "no active step to annotate".to_string(),
            ));
        }
        let occurred_at = now_utc_rfc3339();
        let updated_history = annotate_step_history(&current.step_history, actor, &occurred_at);
        self.write_step_annotate_cache(&id, &current, &updated_history, &occurred_at)
    }

    fn write_step_annotate_cache(
        &self,
        id: &str,
        current: &db::KnotCacheRecord,
        updated_history: &[crate::domain::step_history::StepRecord],
        occurred_at: &str,
    ) -> Result<KnotView, AppError> {
        let profile = self.resolve_profile_for_record(current)?;
        let knot_type = parse_knot_type(current.knot_type.as_deref());
        let terminal = workflow_runtime::is_terminal_state(
            &self.profile_registry,
            profile.id.as_str(),
            knot_type,
            &current.state,
        )?;
        let (step_metadata, next_step_metadata) = resolve_step_metadata(
            &self.profile_registry,
            &profile.workflow_id,
            profile.id.as_str(),
            knot_type,
            &current.gate_data,
            &current.state,
        )?;
        let index_event_id = new_event_id();
        let idx_event = IndexEvent::with_identity(
            index_event_id.clone(),
            occurred_at.to_string(),
            IndexEventKind::KnotHead.as_str(),
            build_knot_head_data(KnotHeadData {
                knot_id: id,
                title: &current.title,
                state: &current.state,
                workflow_id: &profile.workflow_id,
                profile_id: profile.id.as_str(),
                updated_at: occurred_at,
                terminal,
                deferred_from_state: current.deferred_from_state.as_deref(),
                blocked_from_state: current.blocked_from_state.as_deref(),
                invariants: &current.invariants,
                knot_type,
                gate_data: &current.gate_data,
                step_metadata: step_metadata.as_ref(),
                next_step_metadata: next_step_metadata.as_ref(),
            }),
        );
        self.writer.write(&EventRecord::index(idx_event))?;
        db::upsert_knot_hot(
            &self.conn,
            &UpsertKnotHot {
                id,
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
                step_history: updated_history,
                gate_data: &current.gate_data,
                lease_data: &current.lease_data,
                lease_id: current.lease_id.as_deref(),
                workflow_id: &profile.workflow_id,
                profile_id: &profile.id,
                profile_etag: Some(&index_event_id),
                deferred_from_state: current.deferred_from_state.as_deref(),
                blocked_from_state: current.blocked_from_state.as_deref(),
                created_at: current.created_at.as_deref(),
            },
        )?;
        let updated =
            db::get_knot_hot(&self.conn, id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
        self.apply_alias_and_enrich_knot(KnotView::from(updated))
    }

    fn resolve_lease_agent(&self, view: &mut KnotView) {
        let Some(lease_id) = view.lease_id.as_deref() else {
            return;
        };
        if let Ok(Some(lease_record)) = db::get_knot_hot(&self.conn, lease_id) {
            view.lease_agent = Some(LeaseAgentView::from_lease_data(&lease_record.lease_data));
        }
    }

    pub fn cold_sync(&self) -> Result<crate::sync::SyncSummary, AppError> {
        self.pull()
    }

    pub fn cold_search(&self, term: &str) -> Result<Vec<ColdKnotView>, AppError> {
        self.maybe_auto_sync_for_read()?;
        Ok(crate::trace::measure("search_cold_catalog", || {
            db::search_cold_catalog(&self.conn, term)
        })?
        .into_iter()
        .map(|r| ColdKnotView {
            id: r.id,
            title: r.title,
            state: r.state,
            updated_at: r.updated_at,
        })
        .collect())
    }

    pub fn rehydrate(&self, id: &str) -> Result<Option<KnotView>, AppError> {
        let id = self.resolve_knot_token(id)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        if let Some(knot) = db::get_knot_hot(&self.conn, &id)? {
            return Ok(Some(
                self.apply_alias_and_enrich_knot(KnotView::from(knot))?,
            ));
        }
        self.rehydrate_from_warm_cold(&id)
    }

    fn rehydrate_from_warm_cold(&self, id: &str) -> Result<Option<KnotView>, AppError> {
        let warm = db::get_knot_warm(&self.conn, id)?;
        let cold = db::get_cold_catalog(&self.conn, id)?;
        let title = warm
            .as_ref()
            .map(|r| r.title.clone())
            .or_else(|| cold.as_ref().map(|r| r.title.clone()));
        let Some(title) = title else {
            return Ok(None);
        };
        let state = cold
            .as_ref()
            .map(|r| r.state.clone())
            .unwrap_or_else(|| "ready_for_implementation".to_string());
        let updated_at = cold
            .as_ref()
            .map(|r| r.updated_at.clone())
            .unwrap_or_else(crate::events::now_utc_rfc3339);
        let record = rehydrate_from_events(&self.store_paths.root, id, title, state, updated_at)?;
        db::upsert_knot_hot(
            &self.conn,
            &UpsertKnotHot {
                id,
                title: &record.title,
                state: &record.state,
                updated_at: &record.updated_at,
                body: record.body.as_deref(),
                description: record.description.as_deref(),
                acceptance: record.acceptance.as_deref(),
                priority: record.priority,
                knot_type: Some(record.knot_type.as_str()),
                tags: &record.tags,
                notes: &record.notes,
                handoff_capsules: &record.handoff_capsules,
                invariants: &record.invariants,
                step_history: &record.step_history,
                gate_data: &record.gate_data,
                lease_data: &record.lease_data,
                lease_id: record.lease_id.as_deref(),
                workflow_id: &record.workflow_id,
                profile_id: &record.profile_id,
                profile_etag: record.profile_etag.as_deref(),
                deferred_from_state: record.deferred_from_state.as_deref(),
                blocked_from_state: record.blocked_from_state.as_deref(),
                created_at: record.created_at.as_deref(),
            },
        )?;
        let hot =
            db::get_knot_hot(&self.conn, id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
        Ok(Some(self.apply_alias_and_enrich_knot(KnotView::from(hot))?))
    }
}
