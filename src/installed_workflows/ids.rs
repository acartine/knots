use crate::profile::normalize_profile_id;

pub fn normalize_workflow_id(workflow_id: &str) -> String {
    normalize_profile_id(workflow_id).unwrap_or_else(|| workflow_id.trim().to_ascii_lowercase())
}
