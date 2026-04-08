use crate::domain::knot_type::KnotType;
use crate::installed_workflows;
use crate::workflow::ProfileRegistry;

use super::error::AppError;
use super::App;

impl App {
    pub fn default_profile_id(&self) -> Result<String, AppError> {
        let wf = self.default_workflow_id()?;
        self.default_profile_id_for_workflow(&wf)
    }

    pub fn default_workflow_id_for_knot_type(
        &self,
        knot_type: KnotType,
    ) -> Result<String, AppError> {
        match knot_type {
            KnotType::Explore => Ok(installed_workflows::BUILTIN_WORKFLOW_ID.to_string()),
            _ => self.default_workflow_id(),
        }
    }

    pub fn default_profile_id_for_knot_type(
        &self,
        knot_type: KnotType,
    ) -> Result<String, AppError> {
        match knot_type {
            KnotType::Explore => Ok(self.profile_registry.require("exploration")?.id.clone()),
            _ => self.default_profile_id(),
        }
    }

    pub fn set_default_profile_id(&self, profile_id: &str) -> Result<String, AppError> {
        let wf = self.default_workflow_id()?;
        let resolved = self.resolve_profile_id(profile_id, Some(&wf))?;
        let profile = self.profile_registry.require(&resolved)?;
        installed_workflows::set_workflow_default_profile(
            &self.repo_root,
            &wf,
            Some(profile.id.as_str()),
        )?;
        Ok(profile.id.clone())
    }

    pub fn default_profile_id_for_workflow(&self, workflow_id: &str) -> Result<String, AppError> {
        default_profile_id_for_workflow_inner(self, workflow_id)
    }

    pub fn default_quick_profile_id(&self) -> Result<String, AppError> {
        let config = self.read_user_config()?;
        if let Some(id) = self.resolve_config_profile(&config.default_quick_profile) {
            return Ok(id);
        }
        if !installed_workflows::is_builtin_workflow_id(&self.current_workflow_id()?) {
            return self.default_profile_id();
        }
        let profiles = self.profile_registry.list();
        for profile in &profiles {
            if profile.planning_mode == crate::workflow::GateMode::Skipped {
                return Ok(profile.id.clone());
            }
        }
        self.fallback_profile_id()
    }

    pub fn set_default_quick_profile_id(&self, profile_id: &str) -> Result<String, AppError> {
        let resolved = self.resolve_profile_id(profile_id, None)?;
        let profile = self.profile_registry.require(&resolved)?;
        let mut config = self.read_user_config()?;
        config.default_quick_profile = Some(profile.id.clone());
        self.write_user_config(&config)?;
        Ok(profile.id.clone())
    }

    pub(crate) fn profile_registry(&self) -> &ProfileRegistry {
        &self.profile_registry
    }

    pub(crate) fn resolve_profile_id(
        &self,
        raw_profile_id: &str,
        workflow_id: Option<&str>,
    ) -> Result<String, AppError> {
        resolve_profile_id_inner(&self.profile_registry, raw_profile_id, workflow_id)
    }
}

pub(crate) fn resolve_profile_id_inner(
    registry: &ProfileRegistry,
    raw_profile_id: &str,
    workflow_id: Option<&str>,
) -> Result<String, AppError> {
    let workflow_id = workflow_id.map(installed_workflows::canonical_workflow_id);
    let workflow_id = workflow_id.as_deref();
    if let Some(wf) = workflow_id {
        let namespaced = installed_workflows::namespaced_profile_id(wf, raw_profile_id);
        if let Ok(p) = registry.require(&namespaced) {
            return Ok(p.id.clone());
        }
    }
    if let Ok(p) = registry.require(raw_profile_id) {
        if workflow_id.is_none_or(|id| p.workflow_id == id) {
            return Ok(p.id.clone());
        }
    }
    resolve_slash_profile(registry, raw_profile_id, workflow_id)?
        .map_or_else(|| resolve_final(registry, raw_profile_id, workflow_id), Ok)
}

fn resolve_slash_profile(
    registry: &ProfileRegistry,
    raw_profile_id: &str,
    workflow_id: Option<&str>,
) -> Result<Option<String>, AppError> {
    let Some((prefix, suffix)) = raw_profile_id.rsplit_once('/') else {
        return Ok(None);
    };
    if let Some(wf) = workflow_id {
        if prefix == wf
            || (wf == installed_workflows::BUILTIN_WORKFLOW_ID && prefix == "compatibility")
        {
            let namespaced = installed_workflows::namespaced_profile_id(wf, suffix);
            if let Ok(p) = registry.require(&namespaced) {
                return Ok(Some(p.id.clone()));
            }
        }
    } else if let Ok(p) = registry.require(raw_profile_id) {
        return Ok(Some(p.id.clone()));
    } else if let Ok(p) = registry.require(suffix) {
        if installed_workflows::is_builtin_workflow_id(&p.workflow_id) {
            return Ok(Some(p.id.clone()));
        }
    }
    Ok(None)
}

fn resolve_final(
    registry: &ProfileRegistry,
    raw_profile_id: &str,
    workflow_id: Option<&str>,
) -> Result<String, AppError> {
    let p = registry.require(raw_profile_id)?;
    if let Some(wf) = workflow_id {
        if p.workflow_id != wf {
            return Err(AppError::InvalidArgument(format!(
                "profile '{}' does not belong to workflow '{}'",
                p.id, wf
            )));
        }
    }
    Ok(p.id.clone())
}

fn default_profile_id_for_workflow_inner(app: &App, workflow_id: &str) -> Result<String, AppError> {
    let config = app.read_user_config()?;
    if let Some(id) = app.resolve_config_profile(&config.default_profile) {
        let profile = app.profile_registry.require(&id)?;
        if profile.workflow_id == workflow_id {
            return Ok(id);
        }
    }
    if let Ok(registry) = installed_workflows::InstalledWorkflowRegistry::load(&app.repo_root) {
        if let Some(result) = try_registry_default(app, &registry, workflow_id)? {
            return Ok(result);
        }
    }
    if installed_workflows::is_builtin_workflow_id(workflow_id) {
        return app.fallback_profile_id();
    }
    Err(AppError::InvalidArgument(format!(
        "workflow '{}' has no available profiles",
        workflow_id
    )))
}

fn try_registry_default(
    app: &App,
    registry: &installed_workflows::InstalledWorkflowRegistry,
    workflow_id: &str,
) -> Result<Option<String>, AppError> {
    if let Some(pid) = registry.default_profile_id_for_workflow(workflow_id) {
        if app.profile_registry.require(&pid).is_ok() {
            return Ok(Some(pid));
        }
    }
    if let Ok(wf) = registry.require_workflow(workflow_id) {
        if let Some(dp) = wf.default_profile.as_deref() {
            return Ok(Some(app.resolve_profile_id(dp, Some(workflow_id))?));
        }
        if let Some(p) = wf.list_profiles().into_iter().next() {
            return Ok(Some(app.resolve_profile_id(&p.id, Some(workflow_id))?));
        }
    }
    Ok(None)
}
