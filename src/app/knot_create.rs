use std::time::Duration;

use serde_json::json;

use crate::db::{self, UpsertKnotHot};
use crate::domain::knot_type::KnotType;
use crate::events::{
    new_event_id, now_utc_rfc3339, EventRecord, FullEvent, FullEventKind, IndexEvent,
    IndexEventKind,
};
use crate::locks::FileLock;
use crate::workflow_runtime;

use super::error::AppError;
use super::helpers::{
    build_knot_head_data, non_empty, normalize_state_input, require_state_for_knot_type,
    resolve_step_metadata, KnotHeadData,
};
use super::types::{CreateKnotOptions, KnotView};
use super::App;

impl App {
    pub fn create_knot(
        &self,
        title: &str,
        body: Option<&str>,
        initial_state: Option<&str>,
        profile_id: Option<&str>,
    ) -> Result<KnotView, AppError> {
        self.create_knot_in_workflow(title, body, initial_state, profile_id, None)
    }

    pub fn create_knot_in_workflow(
        &self,
        title: &str,
        body: Option<&str>,
        initial_state: Option<&str>,
        profile_id: Option<&str>,
        workflow_id: Option<&str>,
    ) -> Result<KnotView, AppError> {
        self.create_knot_with_options(
            title,
            body,
            initial_state,
            profile_id,
            workflow_id,
            CreateKnotOptions::default(),
        )
    }

    pub fn create_knot_with_options(
        &self,
        title: &str,
        body: Option<&str>,
        initial_state: Option<&str>,
        profile_id: Option<&str>,
        workflow_id: Option<&str>,
        options: CreateKnotOptions,
    ) -> Result<KnotView, AppError> {
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        let (profile, state) =
            self.resolve_create_params(profile_id, workflow_id, initial_state, options.knot_type)?;
        require_state_for_knot_type(options.knot_type, profile, &state)?;
        self.write_create_knot_events(title, body, &state, profile, &options)
    }

    fn resolve_create_params(
        &self,
        profile_id: Option<&str>,
        workflow_id: Option<&str>,
        initial_state: Option<&str>,
        knot_type: KnotType,
    ) -> Result<(&crate::workflow::ProfileDefinition, String), AppError> {
        let resolved_wf = match workflow_id {
            Some(id) => Some(id.to_string()),
            None if profile_id.is_none() => Some(self.default_workflow_id()?),
            None => None,
        };
        let default_profile = if profile_id.is_none() {
            Some(match resolved_wf.as_deref() {
                Some(wf) => self.default_profile_id_for_workflow(wf)?,
                None => self.default_profile_id()?,
            })
        } else {
            None
        };
        let resolved_profile = match profile_id {
            Some(raw) => Some(self.resolve_profile_id(raw, resolved_wf.as_deref())?),
            None => default_profile,
        };
        let profile = self.profile_registry.resolve(resolved_profile.as_deref())?;
        if let Some(wf) = resolved_wf.as_deref() {
            if profile.workflow_id != wf {
                return Err(AppError::InvalidArgument(format!(
                    "profile '{}' does not belong to workflow '{}'",
                    profile.id, wf
                )));
            }
        }
        let state = if let Some(requested) = non_empty(initial_state.unwrap_or("")) {
            normalize_state_input(&requested)?
        } else {
            workflow_runtime::initial_state(knot_type, profile)
        };
        Ok((profile, state))
    }

    fn write_create_knot_events(
        &self,
        title: &str,
        body: Option<&str>,
        state: &str,
        profile: &crate::workflow::ProfileDefinition,
        options: &CreateKnotOptions,
    ) -> Result<KnotView, AppError> {
        let knot_id = self.next_knot_id()?;
        let occurred_at = now_utc_rfc3339();
        let terminal = workflow_runtime::is_terminal_state(
            &self.profile_registry,
            profile.id.as_str(),
            options.knot_type,
            state,
        )?;
        let acceptance = options.acceptance.as_deref().and_then(non_empty);
        let (step_metadata, next_step_metadata) = resolve_step_metadata(
            &self.profile_registry,
            profile.workflow_id.as_str(),
            profile.id.as_str(),
            options.knot_type,
            &options.gate_data,
            state,
        )?;
        let full_event = FullEvent::with_identity(
            new_event_id(),
            occurred_at.clone(),
            knot_id.clone(),
            FullEventKind::KnotCreated.as_str(),
            json!({
                "title": title,
                "state": state,
                "workflow_id": profile.workflow_id.as_str(),
                "profile_id": profile.id.as_str(),
                "body": body,
                "type": options.knot_type.as_str(),
                "gate": &options.gate_data,
            }),
        );
        let index_event_id = new_event_id();
        let idx_event = IndexEvent::with_identity(
            index_event_id.clone(),
            occurred_at.clone(),
            IndexEventKind::KnotHead.as_str(),
            build_knot_head_data(KnotHeadData {
                knot_id: &knot_id,
                title,
                state,
                workflow_id: profile.workflow_id.as_str(),
                profile_id: profile.id.as_str(),
                updated_at: &occurred_at,
                terminal,
                deferred_from_state: None,
                blocked_from_state: None,
                invariants: &[],
                knot_type: options.knot_type,
                gate_data: &options.gate_data,
                step_metadata: step_metadata.as_ref(),
                next_step_metadata: next_step_metadata.as_ref(),
            }),
        );
        self.writer.write(&EventRecord::full(full_event))?;
        self.write_optional_create_events(&knot_id, &occurred_at, acceptance.as_deref(), options)?;
        self.writer.write(&EventRecord::index(idx_event))?;
        db::upsert_knot_hot(
            &self.conn,
            &UpsertKnotHot {
                id: &knot_id,
                title,
                state,
                updated_at: &occurred_at,
                body,
                description: body,
                acceptance: acceptance.as_deref(),
                priority: None,
                knot_type: Some(options.knot_type.as_str()),
                tags: &[],
                notes: &[],
                handoff_capsules: &[],
                invariants: &[],
                step_history: &[],
                gate_data: &options.gate_data,
                lease_data: &options.lease_data,
                lease_id: None,
                workflow_id: profile.workflow_id.as_str(),
                profile_id: profile.id.as_str(),
                profile_etag: Some(&index_event_id),
                deferred_from_state: None,
                blocked_from_state: None,
                created_at: Some(&occurred_at),
            },
        )?;
        let record = db::get_knot_hot(&self.conn, &knot_id)?
            .ok_or_else(|| AppError::NotFound(knot_id.clone()))?;
        self.apply_alias_and_enrich_knot(KnotView::from(record))
    }

    fn write_optional_create_events(
        &self,
        knot_id: &str,
        occurred_at: &str,
        acceptance: Option<&str>,
        options: &CreateKnotOptions,
    ) -> Result<(), AppError> {
        if let Some(acceptance) = acceptance {
            let event = FullEvent::with_identity(
                new_event_id(),
                occurred_at.to_string(),
                knot_id.to_string(),
                FullEventKind::KnotAcceptanceSet.as_str(),
                json!({ "acceptance": acceptance }),
            );
            self.writer.write(&EventRecord::full(event))?;
        }
        if options.knot_type == KnotType::Lease {
            let event = FullEvent::new(
                knot_id.to_string(),
                FullEventKind::KnotLeaseDataSet,
                json!({"lease_data": &options.lease_data}),
            );
            self.writer.write(&EventRecord::full(event))?;
        }
        Ok(())
    }
}
