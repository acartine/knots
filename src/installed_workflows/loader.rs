use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::profile::ProfileError;

use super::{parse_bundle, BundleFormat, WorkflowDefinition, DEFAULT_BUNDLE_FILE};

pub(super) fn load_disk_workflows(
    root: &Path,
    workflows: &mut BTreeMap<String, BTreeMap<u32, WorkflowDefinition>>,
) -> Result<(), ProfileError> {
    let mut entries = fs::read_dir(root)
        .map_err(|e| ProfileError::InvalidBundle(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| ProfileError::InvalidBundle(e.to_string()))?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        load_versions(&path, &id, workflows)?;
    }
    Ok(())
}

fn load_versions(
    path: &Path,
    workflow_id: &str,
    workflows: &mut BTreeMap<String, BTreeMap<u32, WorkflowDefinition>>,
) -> Result<(), ProfileError> {
    let mut entries = fs::read_dir(path)
        .map_err(|e| ProfileError::InvalidBundle(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| ProfileError::InvalidBundle(e.to_string()))?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let vp = entry.path();
        if !vp.is_dir() {
            continue;
        }
        let Some(name) = vp.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Ok(version) = name.parse::<u32>() else {
            continue;
        };
        let Some(bp) = installed_bundle_path(&vp) else {
            continue;
        };
        let raw =
            fs::read_to_string(&bp).map_err(|e| ProfileError::InvalidBundle(e.to_string()))?;
        let format = match bp.extension().and_then(|ext| ext.to_str()) {
            Some("json") => BundleFormat::Json,
            _ => BundleFormat::Toml,
        };
        let wf = parse_bundle(&raw, format)?;
        workflows
            .entry(workflow_id.to_string())
            .or_default()
            .insert(version, wf);
    }
    Ok(())
}

pub(crate) fn installed_bundle_path(workflow_dir: &Path) -> Option<PathBuf> {
    let bundle = workflow_dir.join(DEFAULT_BUNDLE_FILE);
    bundle.exists().then_some(bundle)
}
