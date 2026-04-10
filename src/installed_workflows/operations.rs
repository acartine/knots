use std::fs;
use std::path::{Path, PathBuf};

use crate::profile::ProfileError;

use super::bundle_toml::render_json_bundle_from_toml;
use super::knot_type_registry::WorkflowRef;
use super::loom::{CommandLoomBundleBuilder, LoomBundleBuilder};
use super::{
    parse_bundle, BundleFormat, InstalledWorkflowRegistry, WorkflowDefinition, WorkflowRepoConfig,
    DEFAULT_BUNDLE_FILE, TOML_BUNDLE_FILE,
};

pub fn repo_config_path(repo_root: &Path) -> PathBuf {
    super::workflows_root(repo_root).join("current")
}

pub fn read_repo_config(repo_root: &Path) -> Result<WorkflowRepoConfig, ProfileError> {
    let path = repo_config_path(repo_root);
    if !path.exists() {
        return Ok(WorkflowRepoConfig::default());
    }
    let raw = fs::read_to_string(&path).map_err(|e| ProfileError::InvalidBundle(e.to_string()))?;
    let input: RepoConfigMigrationInput =
        toml::from_str(&raw).map_err(|e| ProfileError::InvalidBundle(e.to_string()))?;
    if repo_config_requires_migration(&input) {
        return Err(ProfileError::InvalidBundle(
            "workflow config requires migration; run workflow install/doctor fix".to_string(),
        ));
    }
    Ok(WorkflowRepoConfig {
        knot_type_workflows: input.knot_type_workflows,
        default_profiles: input.default_profiles,
    }
    .normalize())
}

pub fn write_repo_config(
    repo_root: &Path,
    config: &WorkflowRepoConfig,
) -> Result<(), ProfileError> {
    let path = repo_config_path(repo_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| ProfileError::InvalidBundle(e.to_string()))?;
    }
    let rendered = toml::to_string_pretty(&config.clone().normalize())
        .map_err(|e| ProfileError::InvalidBundle(e.to_string()))?;
    fs::write(path, rendered).map_err(|e| ProfileError::InvalidBundle(e.to_string()))
}

pub fn install_bundle(repo_root: &Path, source: &Path) -> Result<String, ProfileError> {
    install_bundle_with_builder(repo_root, source, &CommandLoomBundleBuilder)
}

pub(crate) fn install_bundle_with_builder(
    repo_root: &Path,
    source: &Path,
    loom_builder: &dyn LoomBundleBuilder,
) -> Result<String, ProfileError> {
    ensure_builtin_workflows_registered(repo_root)?;
    let (raw, format) = read_bundle_source_with_builder(source, loom_builder)?;
    let workflow = parse_bundle(&raw, format)?;
    let canonical_json = match format {
        BundleFormat::Json => raw.clone(),
        BundleFormat::Toml => render_json_bundle_from_toml(&raw)?,
    };
    write_bundle_to_disk(repo_root, &workflow, &canonical_json)?;
    if matches!(format, BundleFormat::Toml) {
        write_toml_copy(repo_root, &workflow, &raw)?;
    }
    Ok(workflow.id)
}

fn write_bundle_to_disk(
    repo_root: &Path,
    workflow: &WorkflowDefinition,
    canonical_json: &str,
) -> Result<(), ProfileError> {
    let workflow_dir = super::workflows_root(repo_root).join(&workflow.id);
    let target_dir = workflow_dir.join(workflow.version.to_string());
    fs::create_dir_all(&target_dir).map_err(|e| ProfileError::InvalidBundle(e.to_string()))?;
    fs::write(target_dir.join(DEFAULT_BUNDLE_FILE), canonical_json)
        .map_err(|e| ProfileError::InvalidBundle(e.to_string()))?;
    fs::write(workflow_dir.join(DEFAULT_BUNDLE_FILE), canonical_json)
        .map_err(|e| ProfileError::InvalidBundle(e.to_string()))
}

fn write_toml_copy(
    repo_root: &Path,
    workflow: &WorkflowDefinition,
    raw: &str,
) -> Result<(), ProfileError> {
    let workflow_dir = super::workflows_root(repo_root).join(&workflow.id);
    let target_dir = workflow_dir.join(workflow.version.to_string());
    fs::write(target_dir.join(TOML_BUNDLE_FILE), raw)
        .map_err(|e| ProfileError::InvalidBundle(e.to_string()))?;
    fs::write(workflow_dir.join(TOML_BUNDLE_FILE), raw)
        .map_err(|e| ProfileError::InvalidBundle(e.to_string()))
}

pub fn namespaced_profile_id(workflow_id: &str, profile_id: &str) -> String {
    format!("{workflow_id}/{profile_id}")
}

pub fn set_current_workflow_selection(
    repo_root: &Path,
    workflow_id: &str,
    version: Option<u32>,
    profile_id: Option<&str>,
) -> Result<WorkflowRepoConfig, ProfileError> {
    set_current_workflow_selection_for_knot_type(
        repo_root,
        crate::domain::knot_type::KnotType::Work,
        workflow_id,
        version,
        profile_id,
    )
}

pub fn set_current_workflow_selection_for_knot_type(
    repo_root: &Path,
    knot_type: crate::domain::knot_type::KnotType,
    workflow_id: &str,
    version: Option<u32>,
    profile_id: Option<&str>,
) -> Result<WorkflowRepoConfig, ProfileError> {
    ensure_builtin_workflows_registered(repo_root)?;
    let registry = InstalledWorkflowRegistry::load(repo_root)?;
    let workflow = match version {
        Some(v) => registry.require_workflow_version(workflow_id, v)?,
        None => registry.require_workflow(workflow_id)?,
    };
    let selected = resolve_profile_for_selection(workflow, profile_id)?;
    let selected = if workflow.builtin {
        selected
    } else {
        namespaced_profile_id(&workflow.id, &selected)
    };
    let mut config = read_repo_config(repo_root)?;
    config.register_workflow_for_knot_type(
        knot_type,
        WorkflowRef::new(workflow.id.clone(), Some(workflow.version)),
        true,
    );
    config.set_default_profile(&workflow.id, selected);
    let config = config.normalize();
    write_repo_config(repo_root, &config)?;
    Ok(config)
}

pub fn register_workflow_for_knot_type(
    repo_root: &Path,
    knot_type: crate::domain::knot_type::KnotType,
    workflow_id: &str,
    version: Option<u32>,
    set_default: bool,
) -> Result<WorkflowRepoConfig, ProfileError> {
    ensure_builtin_workflows_registered(repo_root)?;
    let registry = InstalledWorkflowRegistry::load(repo_root)?;
    let workflow = match version {
        Some(v) => registry.require_workflow_version(workflow_id, v)?,
        None => registry.require_workflow(workflow_id)?,
    };
    let mut config = read_repo_config(repo_root)?;
    config.register_workflow_for_knot_type(
        knot_type,
        WorkflowRef::new(workflow.id.clone(), Some(workflow.version)),
        set_default,
    );
    let config = config.normalize();
    write_repo_config(repo_root, &config)?;
    Ok(config)
}

pub fn ensure_builtin_workflows_registered(
    repo_root: &Path,
) -> Result<WorkflowRepoConfig, ProfileError> {
    migrate_legacy_repo_config(repo_root)?;
    migrate_legacy_cache_db(repo_root)?;
    let mut config = read_repo_config(repo_root)?;
    for (knot_type, workflow) in super::builtin::builtin_workflows()? {
        config.register_workflow_for_knot_type(
            knot_type,
            WorkflowRef::new(workflow.id, Some(workflow.version)),
            config
                .current_workflow_ref_for_knot_type(knot_type)
                .is_none(),
        );
    }
    let config = config.normalize();
    write_repo_config(repo_root, &config)?;
    Ok(config)
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct RepoConfigMigrationInput {
    #[serde(default)]
    knot_type_workflows: std::collections::BTreeMap<String, super::KnotTypeWorkflowConfig>,
    #[serde(default)]
    current_workflow: Option<String>,
    #[serde(default)]
    current_version: Option<u32>,
    #[serde(default, alias = "current_profile")]
    current_profile: Option<String>,
    #[serde(default)]
    default_profiles: std::collections::BTreeMap<String, String>,
}

fn repo_config_requires_migration(input: &RepoConfigMigrationInput) -> bool {
    input.current_workflow.is_some()
        || input.current_version.is_some()
        || input.current_profile.is_some()
        || input
            .default_profiles
            .iter()
            .any(|(workflow_id, profile_id)| {
                workflow_id_requires_migration(workflow_id)
                    || profile_id_requires_migration(profile_id)
            })
        || input
            .knot_type_workflows
            .values()
            .any(knot_type_workflow_requires_migration)
}

fn knot_type_workflow_requires_migration(config: &super::KnotTypeWorkflowConfig) -> bool {
    workflow_id_requires_migration(&config.default.workflow_id)
        || config
            .registered
            .iter()
            .any(|workflow| workflow_id_requires_migration(&workflow.workflow_id))
}

fn workflow_id_requires_migration(workflow_id: &str) -> bool {
    matches!(
        super::normalize_workflow_id(workflow_id).as_str(),
        "compatibility" | "knots_sdlc"
    )
}

fn profile_id_requires_migration(profile_id: &str) -> bool {
    let trimmed = profile_id.trim().to_ascii_lowercase();
    matches!(
        trimmed.as_str(),
        "automation_granular"
            | "default"
            | "delivery"
            | "automation"
            | "granular"
            | "human_gate"
            | "human"
            | "coarse"
            | "pr_human_gate"
    ) || trimmed.starts_with("compatibility/")
        || trimmed.starts_with("knots_sdlc/")
}

fn migrate_legacy_repo_config(repo_root: &Path) -> Result<(), ProfileError> {
    let path = repo_config_path(repo_root);
    if !path.exists() {
        return Ok(());
    }
    let raw = fs::read_to_string(&path).map_err(|e| ProfileError::InvalidBundle(e.to_string()))?;
    let input: RepoConfigMigrationInput =
        toml::from_str(&raw).map_err(|e| ProfileError::InvalidBundle(e.to_string()))?;
    if !repo_config_requires_migration(&input) {
        return Ok(());
    }

    let mut config = WorkflowRepoConfig {
        knot_type_workflows: migrate_knot_type_workflows(input.knot_type_workflows),
        default_profiles: std::collections::BTreeMap::new(),
    };
    if let Some(current_workflow) = input.current_workflow {
        let workflow_id = migrate_workflow_id(&current_workflow);
        config.register_workflow_for_knot_type(
            crate::domain::knot_type::KnotType::Work,
            WorkflowRef::new(workflow_id.clone(), input.current_version),
            true,
        );
        if let Some(current_profile) = input.current_profile {
            config.set_default_profile(
                &workflow_id,
                migrate_profile_reference(&workflow_id, &current_profile),
            );
        }
    }
    for (workflow_id, profile_id) in input.default_profiles {
        let workflow_id = migrate_workflow_id(&workflow_id);
        config.set_default_profile(
            &workflow_id,
            migrate_profile_reference(&workflow_id, &profile_id),
        );
    }
    write_repo_config(repo_root, &config.normalize())
}

fn migrate_knot_type_workflows(
    workflows: std::collections::BTreeMap<String, super::KnotTypeWorkflowConfig>,
) -> std::collections::BTreeMap<String, super::KnotTypeWorkflowConfig> {
    workflows
        .into_iter()
        .map(|(knot_type, config)| {
            let default = WorkflowRef::new(
                migrate_workflow_id(&config.default.workflow_id),
                config.default.version,
            );
            let registered = config
                .registered
                .into_iter()
                .map(|workflow| {
                    WorkflowRef::new(migrate_workflow_id(&workflow.workflow_id), workflow.version)
                })
                .collect();
            (
                knot_type,
                super::KnotTypeWorkflowConfig {
                    default,
                    registered,
                }
                .normalize(),
            )
        })
        .collect()
}

fn migrate_workflow_id(workflow_id: &str) -> String {
    let normalized = super::normalize_workflow_id(workflow_id);
    if matches!(normalized.as_str(), "compatibility" | "knots_sdlc") {
        super::builtin_workflow_id_for_knot_type(crate::domain::knot_type::KnotType::Work)
    } else {
        normalized
    }
}

fn migrate_profile_reference(workflow_id: &str, profile_id: &str) -> String {
    let trimmed = profile_id.trim();
    if workflow_id
        == super::builtin_workflow_id_for_knot_type(crate::domain::knot_type::KnotType::Work)
    {
        let suffix = trimmed.rsplit('/').next().unwrap_or(trimmed);
        match suffix.trim().to_ascii_lowercase().as_str() {
            "automation_granular" | "default" | "delivery" | "automation" | "granular" => {
                "autopilot".to_string()
            }
            "human_gate" | "human" | "coarse" | "pr_human_gate" => "semiauto".to_string(),
            other => other.to_string(),
        }
    } else if let Some((prefix, suffix)) = trimmed.rsplit_once('/') {
        format!(
            "{}/{}",
            super::normalize_workflow_id(prefix),
            crate::profile::normalize_profile_id(suffix)
                .unwrap_or_else(|| suffix.trim().to_ascii_lowercase())
        )
    } else {
        crate::profile::normalize_profile_id(trimmed)
            .unwrap_or_else(|| trimmed.to_ascii_lowercase())
    }
}

fn migrate_legacy_cache_db(repo_root: &Path) -> Result<(), ProfileError> {
    let db_path = repo_root.join(".knots").join("cache").join("state.sqlite");
    if !db_path.exists() {
        return Ok(());
    }
    crate::db::open_connection(
        db_path
            .to_str()
            .ok_or_else(|| ProfileError::InvalidBundle("invalid db path".to_string()))?,
    )
    .map(|_| ())
    .map_err(|e| ProfileError::InvalidBundle(e.to_string()))
}

fn resolve_profile_for_selection(
    workflow: &WorkflowDefinition,
    profile_id: Option<&str>,
) -> Result<String, ProfileError> {
    match profile_id {
        Some(id) => Ok(workflow.require_profile(id)?.id.clone()),
        None => Ok(workflow
            .default_profile
            .as_deref()
            .and_then(|dp| workflow.require_profile(dp).ok())
            .map(|p| p.id.clone())
            .or_else(|| {
                workflow
                    .list_profiles()
                    .into_iter()
                    .next()
                    .map(|p| p.id.clone())
            })
            .unwrap_or_else(|| "autopilot".to_string())),
    }
}

pub fn set_workflow_default_profile(
    repo_root: &Path,
    workflow_id: &str,
    profile_id: Option<&str>,
) -> Result<WorkflowRepoConfig, ProfileError> {
    ensure_builtin_workflows_registered(repo_root)?;
    let registry = InstalledWorkflowRegistry::load(repo_root)?;
    let workflow = registry.require_workflow(workflow_id)?;
    let Some(profile_id) = profile_id else {
        return read_repo_config(repo_root);
    };
    let selected = workflow.require_profile(profile_id)?.id.clone();
    let selected = if workflow.builtin {
        selected
    } else {
        namespaced_profile_id(workflow_id, &selected)
    };
    let mut config = read_repo_config(repo_root)?;
    config.set_default_profile(workflow_id, selected);
    let config = config.normalize();
    write_repo_config(repo_root, &config)?;
    Ok(config)
}

#[cfg(test)]
pub(crate) fn read_bundle_source(source: &Path) -> Result<(String, BundleFormat), ProfileError> {
    read_bundle_source_with_builder(source, &CommandLoomBundleBuilder)
}

pub(crate) fn read_bundle_source_with_builder(
    source: &Path,
    loom_builder: &dyn LoomBundleBuilder,
) -> Result<(String, BundleFormat), ProfileError> {
    if source.is_dir() && source.join("loom.toml").exists() {
        return read_loom_bundle(source, loom_builder);
    }
    let source_path = resolve_bundle_source_path(source)?;
    let raw =
        fs::read_to_string(&source_path).map_err(|e| ProfileError::InvalidBundle(e.to_string()))?;
    let format = match source_path.extension().and_then(|ext| ext.to_str()) {
        Some("json") => BundleFormat::Json,
        _ => BundleFormat::Toml,
    };
    Ok((raw, format))
}

fn read_loom_bundle(
    source: &Path,
    loom_builder: &dyn LoomBundleBuilder,
) -> Result<(String, BundleFormat), ProfileError> {
    let raw = loom_builder.build_knots_bundle(source)?;
    Ok((raw, BundleFormat::Json))
}

pub(crate) fn resolve_bundle_source_path(source: &Path) -> Result<PathBuf, ProfileError> {
    if source.is_file() {
        return Ok(source.to_path_buf());
    }
    if !source.is_dir() {
        return Err(ProfileError::InvalidBundle(format!(
            "bundle source '{}' does not exist",
            source.display()
        )));
    }
    let candidates = [
        source.join(DEFAULT_BUNDLE_FILE),
        source.join(TOML_BUNDLE_FILE),
        source.join("workflow.json"),
        source.join("workflow.toml"),
        source.join("dist").join(DEFAULT_BUNDLE_FILE),
        source.join("dist").join(TOML_BUNDLE_FILE),
        source.join("build").join(DEFAULT_BUNDLE_FILE),
        source.join("build").join(TOML_BUNDLE_FILE),
    ];
    for candidate in candidates {
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(ProfileError::InvalidBundle(format!(
        "no Loom bundle found in '{}'; expected bundle.json, \
         bundle.toml, workflow.json, or workflow.toml",
        source.display()
    )))
}
