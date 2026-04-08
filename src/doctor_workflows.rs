use std::path::Path;

use crate::doctor::{DoctorCheck, DoctorStatus};
use crate::installed_workflows::InstalledWorkflowRegistry;

pub fn check_registered_workflows(repo_root: &Path) -> DoctorCheck {
    match InstalledWorkflowRegistry::load(repo_root) {
        Ok(registry) => {
            let detail = crate::domain::knot_type::KnotType::ALL
                .iter()
                .map(|knot_type| {
                    let workflow = registry.current_workflow_id_for_knot_type(*knot_type);
                    format!("{}={workflow}", knot_type.as_str())
                })
                .collect::<Vec<_>>()
                .join(", ");
            DoctorCheck {
                name: "workflow_registry".to_string(),
                status: DoctorStatus::Pass,
                detail,
            }
        }
        Err(err) => DoctorCheck {
            name: "workflow_registry".to_string(),
            status: DoctorStatus::Fail,
            detail: err.to_string(),
        },
    }
}
