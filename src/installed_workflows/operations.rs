use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::profile::ProfileError;

use super::bundle_toml::render_json_bundle_from_toml;
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
    let config: WorkflowRepoConfig =
        toml::from_str(&raw).map_err(|e| ProfileError::InvalidBundle(e.to_string()))?;
    let normalized = config.clone().normalize();
    if normalized != config {
        write_repo_config(repo_root, &normalized)?;
    }
    Ok(normalized)
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
    let (raw, format) = read_bundle_source(source)?;
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
    config.current_workflow = Some(workflow.id.clone());
    config.current_version = Some(workflow.version);
    config.set_default_profile(&workflow.id, selected);
    write_repo_config(repo_root, &config)?;
    Ok(config)
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
    write_repo_config(repo_root, &config)?;
    Ok(config)
}

pub(crate) fn read_bundle_source(source: &Path) -> Result<(String, BundleFormat), ProfileError> {
    if source.is_dir() && source.join("loom.toml").exists() {
        return read_loom_bundle(source);
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

fn read_loom_bundle(source: &Path) -> Result<(String, BundleFormat), ProfileError> {
    let loom_bin = std::env::var("KNOTS_LOOM_BIN").unwrap_or_else(|_| "loom".to_string());
    let output = Command::new(loom_bin)
        .arg("build")
        .arg(source)
        .arg("--emit")
        .arg("knots-bundle")
        .output()
        .map_err(|err| ProfileError::InvalidBundle(format!("failed to execute loom: {err}")))?;
    if !output.status.success() {
        return Err(ProfileError::InvalidBundle(format!(
            "loom build --emit knots-bundle failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let raw = String::from_utf8(output.stdout).map_err(|err| {
        ProfileError::InvalidBundle(format!("invalid UTF-8 bundle output: {err}"))
    })?;
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
