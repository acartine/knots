use std::io;
use std::path::Path;

#[cfg(test)]
pub const BUNDLE_JSON: &str = include_str!("../loom/work_sdlc/dist/bundle.json");

const LOOM_TOML: &str = include_str!("../loom/work_sdlc/loom.toml");
const WORKFLOW_LOOM: &str = include_str!("../loom/work_sdlc/workflow.loom");

const PROFILE_AUTOPILOT: &str = include_str!("../loom/work_sdlc/profiles/autopilot.loom");
const PROFILE_AUTOPILOT_WITH_PR: &str =
    include_str!("../loom/work_sdlc/profiles/autopilot_with_pr.loom");
const PROFILE_AUTOPILOT_NO_PLANNING: &str =
    include_str!("../loom/work_sdlc/profiles/autopilot_no_planning.loom");
const PROFILE_AUTOPILOT_WITH_PR_NO_PLANNING: &str =
    include_str!("../loom/work_sdlc/profiles/autopilot_with_pr_no_planning.loom");
const PROFILE_SEMIAUTO: &str = include_str!("../loom/work_sdlc/profiles/semiauto.loom");
const PROFILE_SEMIAUTO_NO_PLANNING: &str =
    include_str!("../loom/work_sdlc/profiles/semiauto_no_planning.loom");

const FILES: &[(&str, &str)] = &[
    ("loom.toml", LOOM_TOML),
    ("workflow.loom", WORKFLOW_LOOM),
    ("profiles/autopilot.loom", PROFILE_AUTOPILOT),
    ("profiles/autopilot_with_pr.loom", PROFILE_AUTOPILOT_WITH_PR),
    (
        "profiles/autopilot_no_planning.loom",
        PROFILE_AUTOPILOT_NO_PLANNING,
    ),
    (
        "profiles/autopilot_with_pr_no_planning.loom",
        PROFILE_AUTOPILOT_WITH_PR_NO_PLANNING,
    ),
    ("profiles/semiauto.loom", PROFILE_SEMIAUTO),
    (
        "profiles/semiauto_no_planning.loom",
        PROFILE_SEMIAUTO_NO_PLANNING,
    ),
];

/// Return the raw prompt body for the given action state from the embedded bundle.
/// State names correspond directly to prompt names in the work_sdlc bundle.
#[cfg(test)]
pub fn prompt_body_for_state(state: &str) -> Option<String> {
    let workflow = crate::installed_workflows::parse_bundle(
        BUNDLE_JSON,
        crate::installed_workflows::BundleFormat::Json,
    )
    .ok()?;
    workflow.prompts.get(state).map(|p| p.body.clone())
}

pub fn write_builtin_loom_package(dest: &Path) -> io::Result<()> {
    for (relative, content) in FILES {
        let target = dest.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, content)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_bundle_json_is_valid() {
        let workflow = crate::installed_workflows::parse_bundle(
            BUNDLE_JSON,
            crate::installed_workflows::BundleFormat::Json,
        )
        .expect("embedded bundle JSON should parse");
        assert_eq!(workflow.id, "work_sdlc");
        assert_eq!(workflow.version, 1);
        assert_eq!(workflow.default_profile.as_deref(), Some("autopilot"));
        assert!(workflow.profiles.contains_key("autopilot"));
        assert!(workflow.profiles.contains_key("autopilot_with_pr"));
        assert!(workflow.prompts.contains_key("planning"));
        assert!(workflow.prompts.contains_key("implementation"));
    }
}
