use std::collections::BTreeMap;
use std::path::Path;

use crate::domain::knot_type::KnotType;
use crate::profile::ProfileError;

use super::knot_type_registry::WorkflowRef;
use super::{
    builtin, loader, namespaced_profile_id, normalize_workflow_id, read_repo_config,
    workflows_root, WorkflowDefinition, WorkflowRepoConfig,
};

#[derive(Debug, Clone)]
pub struct InstalledWorkflowRegistry {
    workflows: BTreeMap<String, BTreeMap<u32, WorkflowDefinition>>,
    current: Option<WorkflowRepoConfig>,
}

impl InstalledWorkflowRegistry {
    pub fn load(repo_root: &Path) -> Result<Self, ProfileError> {
        let mut workflows: BTreeMap<String, BTreeMap<u32, WorkflowDefinition>> = BTreeMap::new();
        for (knot_type, builtin_workflow) in builtin::builtin_workflows()? {
            workflows
                .entry(builtin_workflow.id.clone())
                .or_default()
                .insert(builtin_workflow.version, builtin_workflow);
            let _ = knot_type;
        }
        let root = workflows_root(repo_root);
        if root.exists() {
            loader::load_disk_workflows(&root, &mut workflows)?;
        }
        let current = read_repo_config(repo_root)?;
        let current = if current.knot_type_workflows.is_empty() {
            super::operations::ensure_builtin_workflows_registered(repo_root)?
        } else {
            current
        };
        let registry = Self {
            workflows,
            current: Some(current),
        };
        registry.validate_knot_type_invariants()?;
        Ok(registry)
    }

    #[cfg(test)]
    pub fn current_workflow_id(&self) -> String {
        self.current_workflow_id_for_knot_type(KnotType::Work)
    }

    #[cfg(test)]
    pub fn current_workflow_version(&self) -> Option<u32> {
        self.current_workflow_ref_for_knot_type(KnotType::Work)
            .version
    }

    pub fn current_profile_id(&self) -> Option<String> {
        self.default_profile_id_for_knot_type(KnotType::Work)
    }

    pub fn current_workflow_id_for_knot_type(&self, knot_type: KnotType) -> String {
        self.current_workflow_ref_for_knot_type(knot_type)
            .workflow_id
    }

    fn configured_workflow_ref_for_knot_type(&self, knot_type: KnotType) -> Option<WorkflowRef> {
        self.current
            .as_ref()
            .and_then(|cfg| cfg.current_workflow_ref_for_knot_type(knot_type))
    }

    pub fn current_workflow_ref_for_knot_type(&self, knot_type: KnotType) -> WorkflowRef {
        self.configured_workflow_ref_for_knot_type(knot_type)
            .expect("workflow registry invariant should guarantee current workflow per knot type")
    }

    pub fn current_workflow_for_knot_type(
        &self,
        knot_type: KnotType,
    ) -> Result<&WorkflowDefinition, ProfileError> {
        let current = self.current_workflow_ref_for_knot_type(knot_type);
        match current.version {
            Some(version) => self.require_workflow_version(&current.workflow_id, version),
            None => self.require_workflow(&current.workflow_id),
        }
    }

    pub fn default_profile_id_for_knot_type(&self, knot_type: KnotType) -> Option<String> {
        let workflow_id = self.current_workflow_id_for_knot_type(knot_type);
        self.default_profile_id_for_workflow(&workflow_id)
    }

    pub fn registered_workflows_for_knot_type(
        &self,
        knot_type: KnotType,
    ) -> Vec<&WorkflowDefinition> {
        self.current
            .as_ref()
            .and_then(|cfg| cfg.knot_type_workflows.get(knot_type.as_str()))
            .map(|entry| {
                entry
                    .registered
                    .iter()
                    .filter_map(|workflow| match workflow.version {
                        Some(version) => self
                            .require_workflow_version(&workflow.workflow_id, version)
                            .ok(),
                        None => self.require_workflow(&workflow.workflow_id).ok(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    }

    pub fn default_profile_id_for_workflow(&self, workflow_id: &str) -> Option<String> {
        let workflow_id = normalize_workflow_id(workflow_id);
        if let Some(pid) = self
            .current
            .as_ref()
            .and_then(|c| c.default_profile_id_for_workflow(&workflow_id))
        {
            return Some(pid.to_string());
        }
        let wf = self.require_workflow(&workflow_id).ok()?;
        let dp = wf
            .default_profile
            .as_deref()
            .or_else(|| wf.profiles.keys().next().map(String::as_str))?;
        if wf.builtin {
            Some(dp.to_string())
        } else {
            Some(namespaced_profile_id(&workflow_id, dp))
        }
    }

    pub fn current_workflow(&self) -> Result<&WorkflowDefinition, ProfileError> {
        self.current_workflow_for_knot_type(KnotType::Work)
    }

    pub fn require_workflow(&self, workflow_id: &str) -> Result<&WorkflowDefinition, ProfileError> {
        let id = normalize_workflow_id(workflow_id);
        self.workflows
            .get(&id)
            .and_then(|v| v.iter().next_back().map(|(_, w)| w))
            .ok_or(ProfileError::UnknownWorkflow(id))
    }

    pub fn require_workflow_version(
        &self,
        workflow_id: &str,
        version: u32,
    ) -> Result<&WorkflowDefinition, ProfileError> {
        let id = normalize_workflow_id(workflow_id);
        self.workflows
            .get(&id)
            .and_then(|v| v.get(&version))
            .ok_or(ProfileError::UnknownWorkflow(id))
    }

    pub fn list(&self) -> Vec<&WorkflowDefinition> {
        let mut r = Vec::new();
        for v in self.workflows.values() {
            r.extend(v.values());
        }
        r.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| a.version.cmp(&b.version)));
        r
    }

    pub fn validate_knot_type_invariants(&self) -> Result<(), ProfileError> {
        for knot_type in KnotType::ALL {
            let registered = self.registered_workflows_for_knot_type(knot_type);
            if registered.is_empty() {
                return Err(ProfileError::InvalidBundle(format!(
                    "knot type '{}' has no registered workflows",
                    knot_type
                )));
            }
            let Some(current) = self.configured_workflow_ref_for_knot_type(knot_type) else {
                return Err(ProfileError::InvalidBundle(format!(
                    "knot type '{}' has no default workflow",
                    knot_type
                )));
            };
            let current_exists = match current.version {
                Some(version) => self
                    .require_workflow_version(&current.workflow_id, version)
                    .is_ok(),
                None => self.require_workflow(&current.workflow_id).is_ok(),
            };
            if !current_exists {
                return Err(ProfileError::UnknownWorkflow(current.workflow_id));
            }
        }
        Ok(())
    }
}
