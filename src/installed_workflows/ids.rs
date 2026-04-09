use crate::profile::normalize_profile_id;

pub fn normalize_workflow_id(workflow_id: &str) -> String {
    normalize_profile_id(workflow_id).unwrap_or_else(|| workflow_id.trim().to_ascii_lowercase())
}

pub fn canonicalize_persisted_workflow_id(workflow_id: &str) -> String {
    let normalized = normalize_workflow_id(workflow_id);
    if matches!(normalized.as_str(), "compatibility" | "knots_sdlc") {
        super::builtin_workflow_id_for_knot_type(crate::domain::knot_type::KnotType::Work)
    } else {
        normalized
    }
}
