#![allow(dead_code)]

use std::time::Duration;

use serde_json::json;

use crate::app::error::AppError;
use crate::app::helpers::{build_knot_head_data, resolve_step_metadata, KnotHeadData};
use crate::app::types::KnotView;
use crate::db::{self, UpsertKnotHot};
use crate::domain::execution_plan::ExecutionPlanData;
use crate::domain::execution_plan_edit::{
    add_step, add_wave, move_step, move_wave, remove_step, remove_wave, CascadeInfo, PlanEditError,
};
use crate::domain::knot_type::parse_knot_type;
use crate::events::{
    new_event_id, now_utc_rfc3339, EventRecord, FullEvent, FullEventKind, IndexEvent,
    IndexEventKind,
};
use crate::locks::FileLock;
use crate::workflow_runtime;

use super::App;

impl App {
    pub fn plan_edit_wave_add(
        &self,
        id: &str,
        name: String,
        objective: String,
        at: Option<u32>,
    ) -> Result<KnotView, AppError> {
        let (id, current) = self.load_plan_edit_target(id)?;
        let next =
            add_wave(&current.execution_plan_data, name, objective, at).map_err(plan_edit_error)?;
        self.write_execution_plan_data(&id, &current, next)
    }

    pub fn plan_edit_wave_remove(
        &self,
        id: &str,
        wave_index: u32,
    ) -> Result<(KnotView, CascadeInfo), AppError> {
        let (id, current) = self.load_plan_edit_target(id)?;
        let (next, cascade) =
            remove_wave(&current.execution_plan_data, wave_index).map_err(plan_edit_error)?;
        let knot = self.write_execution_plan_data(&id, &current, next)?;
        Ok((knot, cascade))
    }

    pub fn plan_edit_wave_move(&self, id: &str, from: u32, to: u32) -> Result<KnotView, AppError> {
        let (id, current) = self.load_plan_edit_target(id)?;
        let next = move_wave(&current.execution_plan_data, from, to).map_err(plan_edit_error)?;
        self.write_execution_plan_data(&id, &current, next)
    }

    pub fn plan_edit_step_add(
        &self,
        id: &str,
        wave_index: u32,
        knot_ids: Vec<String>,
        notes: Option<String>,
        at: Option<u32>,
    ) -> Result<KnotView, AppError> {
        let (id, current) = self.load_plan_edit_target(id)?;
        let knot_ids = self.normalize_plan_edit_knot_ids(knot_ids)?;
        let next = add_step(
            &current.execution_plan_data,
            wave_index,
            knot_ids,
            notes,
            at,
        )
        .map_err(plan_edit_error)?;
        self.write_execution_plan_data(&id, &current, next)
    }

    pub fn plan_edit_step_remove(
        &self,
        id: &str,
        wave_index: u32,
        step_index: u32,
    ) -> Result<(KnotView, CascadeInfo), AppError> {
        let (id, current) = self.load_plan_edit_target(id)?;
        let (next, cascade) = remove_step(&current.execution_plan_data, wave_index, step_index)
            .map_err(plan_edit_error)?;
        let knot = self.write_execution_plan_data(&id, &current, next)?;
        Ok((knot, cascade))
    }

    pub fn plan_edit_step_move(
        &self,
        id: &str,
        wave_index: u32,
        from: u32,
        to: u32,
    ) -> Result<KnotView, AppError> {
        let (id, current) = self.load_plan_edit_target(id)?;
        let next = move_step(&current.execution_plan_data, wave_index, from, to)
            .map_err(plan_edit_error)?;
        self.write_execution_plan_data(&id, &current, next)
    }

    fn load_plan_edit_target(&self, id: &str) -> Result<(String, db::KnotCacheRecord), AppError> {
        let id = self.resolve_knot_token(id)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        let current =
            db::get_knot_hot(&self.conn, &id)?.ok_or_else(|| AppError::NotFound(id.clone()))?;
        Ok((id, current))
    }

    fn normalize_plan_edit_knot_ids(&self, knot_ids: Vec<String>) -> Result<Vec<String>, AppError> {
        knot_ids
            .into_iter()
            .map(|token| self.resolve_knot_token_strict(&token))
            .collect()
    }

    fn write_execution_plan_data(
        &self,
        id: &str,
        current: &db::KnotCacheRecord,
        execution_plan_data: ExecutionPlanData,
    ) -> Result<KnotView, AppError> {
        let profile = self.resolve_profile_for_record(current)?;
        let occurred_at = now_utc_rfc3339();
        let knot_type = parse_knot_type(current.knot_type.as_deref());
        let terminal = workflow_runtime::is_terminal_state(
            &self.profile_registry,
            profile.id.as_str(),
            knot_type,
            current.state.as_str(),
        )?;
        let (step_metadata, next_step_metadata) = resolve_step_metadata(
            &self.profile_registry,
            profile.workflow_id.as_str(),
            profile.id.as_str(),
            knot_type,
            &current.gate_data,
            current.state.as_str(),
        )?;

        let full_event = FullEvent::with_identity(
            new_event_id(),
            occurred_at.clone(),
            id.to_string(),
            FullEventKind::KnotExecutionPlanDataSet.as_str(),
            json!({ "execution_plan": &execution_plan_data }),
        );
        let mut full_event = full_event;
        if let Some(expected) = current.profile_etag.as_deref() {
            full_event = full_event.with_precondition(expected);
        }
        self.writer.write(&EventRecord::full(full_event))?;

        let index_event_id = new_event_id();
        let mut idx_event = IndexEvent::with_identity(
            index_event_id.clone(),
            occurred_at.clone(),
            IndexEventKind::KnotHead.as_str(),
            build_knot_head_data(KnotHeadData {
                knot_id: id,
                title: &current.title,
                state: &current.state,
                workflow_id: profile.workflow_id.as_str(),
                profile_id: profile.id.as_str(),
                updated_at: &occurred_at,
                terminal,
                deferred_from_state: current.deferred_from_state.as_deref(),
                blocked_from_state: current.blocked_from_state.as_deref(),
                invariants: &current.invariants,
                knot_type,
                gate_data: &current.gate_data,
                execution_plan_data: &execution_plan_data,
                step_metadata: step_metadata.as_ref(),
                next_step_metadata: next_step_metadata.as_ref(),
            }),
        );
        if let Some(expected) = current.profile_etag.as_deref() {
            idx_event = idx_event.with_precondition(expected);
        }
        self.writer.write(&EventRecord::index(idx_event))?;

        db::upsert_knot_hot(
            &self.conn,
            &UpsertKnotHot {
                id,
                title: &current.title,
                state: &current.state,
                updated_at: &occurred_at,
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
                execution_plan_data: &execution_plan_data,
                lease_id: current.lease_id.as_deref(),
                workflow_id: profile.workflow_id.as_str(),
                profile_id: profile.id.as_str(),
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
}

fn plan_edit_error(error: PlanEditError) -> AppError {
    AppError::InvalidArgument(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use uuid::Uuid;

    use super::*;

    fn unique_workspace(prefix: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&root).expect("temp workspace should be creatable");
        root
    }

    fn run_git(root: &Path, args: &[&str]) {
        let output = Command::new("git").arg("-C").arg(root).args(args).output();
        let output = output.expect("git command should run");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn setup_repo(root: &Path) {
        run_git(root, &["init"]);
        run_git(root, &["config", "user.email", "knots@example.com"]);
        run_git(root, &["config", "user.name", "Knots Test"]);
        std::fs::write(root.join("README.md"), "# knots\n").expect("readme should be writable");
        run_git(root, &["add", "README.md"]);
        run_git(root, &["commit", "-m", "init"]);
        run_git(root, &["branch", "-M", "main"]);
    }

    fn read_event_types(root: &Path) -> Vec<String> {
        fn walk(dir: &Path, types: &mut Vec<String>) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        walk(&path, types);
                    } else if path.extension().is_some_and(|ext| ext == "json") {
                        let bytes = std::fs::read(&path).expect("event file");
                        let value: serde_json::Value =
                            serde_json::from_slice(&bytes).expect("json");
                        if let Some(event_type) = value.get("type").and_then(|v| v.as_str()) {
                            types.push(event_type.to_string());
                        }
                    }
                }
            }
        }

        let mut types = Vec::new();
        walk(&root.join(".knots/events"), &mut types);
        types
    }

    #[test]
    fn plan_wave_add_persists_and_preserves_fields() {
        let root = unique_workspace("knots-plan-edit-wave-add");
        setup_repo(&root);
        let db = root.join(".knots/cache/state.sqlite");
        let app = App::open(db.to_str().expect("utf8"), root.clone()).expect("app");
        let target = app
            .create_knot("Plan", None, Some("idea"), Some("default"))
            .expect("knot");
        let ref_knot = app
            .create_knot("Ref", None, Some("idea"), Some("default"))
            .expect("ref");
        let updated = app
            .plan_edit_wave_add(&target.id, "Wave 1".to_string(), "First".to_string(), None)
            .expect("wave add");
        assert_eq!(updated.execution_plan.as_ref().unwrap().waves.len(), 1);
        assert_eq!(
            updated.execution_plan.as_ref().unwrap().waves[0].wave_index,
            1
        );
        assert!(read_event_types(&root)
            .iter()
            .any(|event_type| event_type == "knot.execution_plan_data_set"));

        let updated = app
            .plan_edit_step_add(
                &target.id,
                1,
                vec![ref_knot.id.clone()],
                Some("notes".to_string()),
                None,
            )
            .expect("step add");
        let plan = updated.execution_plan.expect("plan");
        assert_eq!(plan.waves[0].steps.len(), 1);
        assert_eq!(plan.waves[0].steps[0].knot_ids, vec![ref_knot.id]);
        assert_eq!(plan.waves[0].steps[0].notes.as_deref(), Some("notes"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn plan_wave_remove_returns_cascade_info() {
        let root = unique_workspace("knots-plan-edit-wave-remove");
        setup_repo(&root);
        let db = root.join(".knots/cache/state.sqlite");
        let app = App::open(db.to_str().expect("utf8"), root.clone()).expect("app");
        let target = app
            .create_knot("Plan", None, Some("idea"), Some("default"))
            .expect("knot");
        let ref_a = app
            .create_knot("Ref A", None, Some("idea"), Some("default"))
            .expect("a");
        let ref_b = app
            .create_knot("Ref B", None, Some("idea"), Some("default"))
            .expect("b");
        let plan = ExecutionPlanData {
            waves: vec![
                crate::domain::execution_plan::ExecutionPlanWave {
                    wave_index: 1,
                    name: "keep".to_string(),
                    objective: "keep".to_string(),
                    ..Default::default()
                },
                crate::domain::execution_plan::ExecutionPlanWave {
                    wave_index: 2,
                    name: "drop".to_string(),
                    objective: "drop".to_string(),
                    knots: vec![crate::domain::execution_plan::ExecutionPlanKnot {
                        id: ref_a.id.clone(),
                        title: "a".to_string(),
                    }],
                    steps: vec![crate::domain::execution_plan::ExecutionPlanStep {
                        step_index: 1,
                        knot_ids: vec![ref_a.id.clone(), ref_b.id.clone()],
                        notes: None,
                    }],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        app.update_knot(
            &target.id,
            crate::app::types::UpdateKnotPatch {
                execution_plan_data: Some(plan),
                ..Default::default()
            },
        )
        .expect("seed plan");
        let (updated, cascade) = app
            .plan_edit_wave_remove(&target.id, 2)
            .expect("remove wave");
        assert_eq!(cascade.step_count, 1);
        assert_eq!(
            cascade.affected_knot_ids,
            vec![ref_a.id.clone(), ref_b.id.clone()]
        );
        let plan = updated.execution_plan.expect("plan");
        assert_eq!(plan.waves.len(), 1);
        assert_eq!(plan.waves[0].wave_index, 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn plan_step_move_renumbers_steps() {
        let root = unique_workspace("knots-plan-edit-step-move");
        setup_repo(&root);
        let db = root.join(".knots/cache/state.sqlite");
        let app = App::open(db.to_str().expect("utf8"), root.clone()).expect("app");
        let target = app
            .create_knot("Plan", None, Some("idea"), Some("default"))
            .expect("knot");
        let ref_a = app
            .create_knot("Ref A", None, Some("idea"), Some("default"))
            .expect("a");
        let ref_b = app
            .create_knot("Ref B", None, Some("idea"), Some("default"))
            .expect("b");
        let plan = ExecutionPlanData {
            waves: vec![crate::domain::execution_plan::ExecutionPlanWave {
                wave_index: 1,
                name: "wave".to_string(),
                objective: "obj".to_string(),
                steps: vec![
                    crate::domain::execution_plan::ExecutionPlanStep {
                        step_index: 1,
                        knot_ids: vec![ref_a.id.clone()],
                        notes: None,
                    },
                    crate::domain::execution_plan::ExecutionPlanStep {
                        step_index: 2,
                        knot_ids: vec![ref_b.id.clone()],
                        notes: None,
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        app.update_knot(
            &target.id,
            crate::app::types::UpdateKnotPatch {
                execution_plan_data: Some(plan),
                ..Default::default()
            },
        )
        .expect("seed plan");
        let updated = app
            .plan_edit_step_move(&target.id, 1, 1, 2)
            .expect("move step");
        let plan = updated.execution_plan.expect("plan");
        assert_eq!(plan.waves[0].steps[0].knot_ids, vec![ref_b.id.clone()]);
        assert_eq!(plan.waves[0].steps[1].knot_ids, vec![ref_a.id.clone()]);
        assert_eq!(plan.waves[0].steps[0].step_index, 1);
        assert_eq!(plan.waves[0].steps[1].step_index, 2);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn plan_edit_error_wraps_as_invalid_argument() {
        let error = plan_edit_error(PlanEditError::WaveNotFound(4));
        assert!(matches!(error, AppError::InvalidArgument(_)));
        assert_eq!(error.to_string(), "wave 4 not found");
    }
}
