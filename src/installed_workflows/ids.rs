use crate::profile::normalize_profile_id;

use super::BUILTIN_WORKFLOW_ID;

pub fn normalize_workflow_id(workflow_id: &str) -> String {
    normalize_profile_id(workflow_id).unwrap_or_else(|| workflow_id.trim().to_ascii_lowercase())
}

pub(crate) fn repair_legacy_builtin_workflow_id(workflow_id: &str) -> String {
    let normalized = normalize_workflow_id(workflow_id);
    if normalized == "compatibility" {
        BUILTIN_WORKFLOW_ID.to_string()
    } else {
        normalized
    }
}

pub(crate) fn repair_profile_reference(workflow_id: &str, profile_id: &str) -> String {
    let trimmed = profile_id.trim();
    if workflow_id == BUILTIN_WORKFLOW_ID {
        let suffix = trimmed.rsplit('/').next().unwrap_or(trimmed);
        normalize_profile_id(suffix).unwrap_or_else(|| suffix.to_ascii_lowercase())
    } else if let Some((prefix, suffix)) = trimmed.rsplit_once('/') {
        let prefix = normalize_workflow_id(prefix);
        let suffix = normalize_profile_id(suffix).unwrap_or_else(|| suffix.to_ascii_lowercase());
        format!("{prefix}/{suffix}")
    } else {
        normalize_profile_id(trimmed).unwrap_or_else(|| trimmed.to_ascii_lowercase())
    }
}
