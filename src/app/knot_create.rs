use std::time::Duration;

use serde_json::json;

use crate::db::{self, UpsertKnotHot};
use crate::domain::knot_type::KnotType;
use crate::events::{
    new_event_id, now_utc_rfc3339, EventRecord, FullEvent, FullEventKind, IndexEvent,
    IndexEventKind,
};
use crate::locks::FileLock;
use crate::workflow::{ProfileDefinition, StepMetadata};
use crate::workflow_runtime;

use super::error::AppError;
use super::helpers::{
    build_knot_head_data, non_empty, normalize_state_input, normalize_tag,
    normalize_verification_steps, require_state_for_knot_type, resolve_step_metadata,
    validate_execution_plan_data_for_knot_type, KnotHeadData,
};
use super::types::{CreateKnotOptions, KnotView};
use super::App;

struct CreateKnotHeadParams<'a> {
    knot_id: &'a str,
    title: &'a str,
    state: &'a str,
    profile: &'a ProfileDefinition,
    options: &'a CreateKnotOptions,
    occurred_at: &'a str,
    terminal: bool,
    step_metadata: Option<&'a StepMetadata>,
    next_step_metadata: Option<&'a StepMetadata>,
}

struct OptionalCreateEvents<'a> {
    knot_id: &'a str,
    occurred_at: &'a str,
    acceptance: Option<&'a str>,
    description: Option<&'a str>,
    options: &'a CreateKnotOptions,
    verification_steps: &'a [String],
}

fn create_knot_head_data(params: CreateKnotHeadParams<'_>) -> serde_json::Value {
    build_knot_head_data(KnotHeadData {
        knot_id: params.knot_id,
        title: params.title,
        state: params.state,
        workflow_id: params.profile.workflow_id.as_str(),
        profile_id: params.profile.id.as_str(),
        updated_at: params.occurred_at,
        terminal: params.terminal,
        deferred_from_state: None,
        blocked_from_state: None,
        invariants: &[],
        knot_type: params.options.knot_type,
        gate_data: &params.options.gate_data,
        execution_plan_data: &params.options.execution_plan_data,
        scope_data: Some(&params.options.scope_data),
        step_metadata: params.step_metadata,
        next_step_metadata: params.next_step_metadata,
        // A brand-new knot's lease expiry (if it is a lease knot at all)
        // is not known until `App::set_lease_expiry` runs after creation;
        // that call emits its own idx.knot_head refresh with the real
        // value, so 0 here is a transient placeholder, not a lie.
        lease_expiry_ts: 0,
    })
}

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
        mut options: CreateKnotOptions,
    ) -> Result<KnotView, AppError> {
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        if !options.execution_plan_data.is_empty() {
            options
                .execution_plan_data
                .normalize_knot_ids(|token| self.resolve_knot_token_strict(token))?;
        }
        validate_execution_plan_data_for_knot_type(
            options.knot_type,
            &options.execution_plan_data,
        )?;
        if let Some(lease_id) = options.lease_id.as_deref() {
            crate::lease_guard::validate_bindable_external_lease(self, lease_id)?;
        }
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
        let use_default_profile =
            profile_id.is_none_or(|raw| raw.trim().eq_ignore_ascii_case("default"));
        let resolved_wf = match workflow_id {
            Some(id) => Some(id.to_string()),
            None if use_default_profile => Some(self.default_workflow_id_for_knot_type(knot_type)?),
            None => None,
        };
        let default_profile = if use_default_profile {
            Some(match resolved_wf.as_deref() {
                Some(wf) => self.default_profile_id_for_workflow(wf)?,
                None => self.default_profile_id_for_knot_type(knot_type)?,
            })
        } else {
            None
        };
        let resolved_profile = match (profile_id, use_default_profile) {
            (_, true) => default_profile,
            (Some(raw), false) => Some(self.resolve_profile_id(raw, resolved_wf.as_deref())?),
            (None, false) => None,
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
        profile: &ProfileDefinition,
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
        let description = body.and_then(non_empty);
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
        let idx_event = IndexEvent::with_identity(
            new_event_id(),
            occurred_at.clone(),
            IndexEventKind::KnotHead.as_str(),
            create_knot_head_data(CreateKnotHeadParams {
                knot_id: &knot_id,
                title,
                state,
                profile,
                options,
                occurred_at: &occurred_at,
                terminal,
                step_metadata: step_metadata.as_ref(),
                next_step_metadata: next_step_metadata.as_ref(),
            }),
        );
        self.writer.write(&EventRecord::full(full_event))?;
        let mut tags = Vec::new();
        let verification_steps = normalize_verification_steps(&options.verification_steps);
        self.write_optional_create_events(
            OptionalCreateEvents {
                knot_id: &knot_id,
                occurred_at: &occurred_at,
                acceptance: acceptance.as_deref(),
                description: description.as_deref(),
                options,
                verification_steps: &verification_steps,
            },
            &mut tags,
        )?;
        self.writer.write(&EventRecord::index(idx_event.clone()))?;
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
                tags: &tags,
                notes: &[],
                handoff_capsules: &[],
                invariants: &[],
                verification_steps: &verification_steps,
                step_history: &[],
                gate_data: &options.gate_data,
                lease_data: &options.lease_data,
                execution_plan_data: &options.execution_plan_data,
                lease_id: options.lease_id.as_deref(),
                lease_expiry_ts: 0,
                workflow_id: profile.workflow_id.as_str(),
                profile_id: profile.id.as_str(),
                profile_etag: Some(&idx_event.event_id),
                deferred_from_state: None,
                blocked_from_state: None,
                created_at: Some(&occurred_at),
            },
        )?;
        db::update_knot_scope_data(&self.conn, &knot_id, &options.scope_data)?;
        let record = db::get_knot_hot(&self.conn, &knot_id)?
            .ok_or_else(|| AppError::NotFound(knot_id.clone()))?;
        self.apply_alias_and_enrich_knot(KnotView::from(record))
    }

    fn write_optional_create_events(
        &self,
        params: OptionalCreateEvents<'_>,
        tags: &mut Vec<String>,
    ) -> Result<(), AppError> {
        if let Some(description) = params.description {
            let event = FullEvent::with_identity(
                new_event_id(),
                params.occurred_at.to_string(),
                params.knot_id.to_string(),
                FullEventKind::KnotDescriptionSet.as_str(),
                json!({ "description": description }),
            );
            self.writer.write(&EventRecord::full(event))?;
        }
        if let Some(acceptance) = params.acceptance {
            let event = FullEvent::with_identity(
                new_event_id(),
                params.occurred_at.to_string(),
                params.knot_id.to_string(),
                FullEventKind::KnotAcceptanceSet.as_str(),
                json!({ "acceptance": acceptance }),
            );
            self.writer.write(&EventRecord::full(event))?;
        }
        if !params.options.scope_data.is_empty() {
            let event = FullEvent::with_identity(
                new_event_id(),
                params.occurred_at.to_string(),
                params.knot_id.to_string(),
                FullEventKind::KnotScopeSet.as_str(),
                serde_json::to_value(&params.options.scope_data)
                    .expect("scope data should serialize"),
            );
            self.writer.write(&EventRecord::full(event))?;
        }
        if !params.verification_steps.is_empty() {
            let event = FullEvent::with_identity(
                new_event_id(),
                params.occurred_at.to_string(),
                params.knot_id.to_string(),
                FullEventKind::KnotVerificationStepsSet.as_str(),
                json!({ "verification_steps": params.verification_steps }),
            );
            self.writer.write(&EventRecord::full(event))?;
        }
        for raw_tag in &params.options.tags {
            let normalized = normalize_tag(raw_tag);
            if normalized.is_empty() {
                continue;
            }
            if !tags.iter().any(|tag| tag.eq_ignore_ascii_case(&normalized)) {
                tags.push(normalized.clone());
                self.writer
                    .write(&EventRecord::full(FullEvent::with_identity(
                        new_event_id(),
                        params.occurred_at.to_string(),
                        params.knot_id.to_string(),
                        FullEventKind::KnotTagAdd.as_str(),
                        json!({"tag": normalized}),
                    )))?;
            }
        }
        if params.options.knot_type == KnotType::Lease {
            let event = FullEvent::new(
                params.knot_id.to_string(),
                FullEventKind::KnotLeaseDataSet,
                json!({"lease_data": &params.options.lease_data}),
            );
            self.writer.write(&EventRecord::full(event))?;
        }
        if let Some(lease_id) = params.options.lease_id.as_deref() {
            let event = FullEvent::with_identity(
                new_event_id(),
                params.occurred_at.to_string(),
                params.knot_id.to_string(),
                FullEventKind::KnotLeaseIdSet.as_str(),
                json!({ "lease_id": lease_id }),
            );
            self.writer.write(&EventRecord::full(event))?;
        }
        if !params.options.execution_plan_data.is_empty() {
            let event = FullEvent::new(
                params.knot_id.to_string(),
                FullEventKind::KnotExecutionPlanDataSet,
                json!({"execution_plan": &params.options.execution_plan_data}),
            );
            self.writer.write(&EventRecord::full(event))?;
        }
        Ok(())
    }
}
