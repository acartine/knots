use std::io;
use std::path::Path;

const LOOM_TOML: &str = include_str!("../loom/knots_sdlc/loom.toml");
const WORKFLOW_LOOM: &str = include_str!("../loom/knots_sdlc/workflow.loom");

const PROFILE_AUTOPILOT: &str = include_str!("../loom/knots_sdlc/profiles/autopilot.loom");
const PROFILE_AUTOPILOT_WITH_PR: &str =
    include_str!("../loom/knots_sdlc/profiles/autopilot_with_pr.loom");
const PROFILE_AUTOPILOT_NO_PLANNING: &str =
    include_str!("../loom/knots_sdlc/profiles/autopilot_no_planning.loom");
const PROFILE_AUTOPILOT_WITH_PR_NO_PLANNING: &str =
    include_str!("../loom/knots_sdlc/profiles/autopilot_with_pr_no_planning.loom");
const PROFILE_SEMIAUTO: &str = include_str!("../loom/knots_sdlc/profiles/semiauto.loom");
const PROFILE_SEMIAUTO_NO_PLANNING: &str =
    include_str!("../loom/knots_sdlc/profiles/semiauto_no_planning.loom");

const PROMPT_PLANNING: &str = include_str!("../loom/knots_sdlc/prompts/planning.md");
const PROMPT_PLAN_REVIEW: &str = include_str!("../loom/knots_sdlc/prompts/plan_review.md");
const PROMPT_IMPLEMENTATION: &str = include_str!("../loom/knots_sdlc/prompts/implementation.md");
const PROMPT_IMPLEMENTATION_REVIEW: &str =
    include_str!("../loom/knots_sdlc/prompts/implementation_review.md");
const PROMPT_SHIPMENT: &str = include_str!("../loom/knots_sdlc/prompts/shipment.md");
const PROMPT_SHIPMENT_REVIEW: &str = include_str!("../loom/knots_sdlc/prompts/shipment_review.md");
const PROMPT_EVALUATING: &str = include_str!("../loom/knots_sdlc/prompts/evaluating.md");

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
    ("prompts/planning.md", PROMPT_PLANNING),
    ("prompts/plan_review.md", PROMPT_PLAN_REVIEW),
    ("prompts/implementation.md", PROMPT_IMPLEMENTATION),
    (
        "prompts/implementation_review.md",
        PROMPT_IMPLEMENTATION_REVIEW,
    ),
    ("prompts/shipment.md", PROMPT_SHIPMENT),
    ("prompts/shipment_review.md", PROMPT_SHIPMENT_REVIEW),
    ("prompts/evaluating.md", PROMPT_EVALUATING),
];

pub fn prompt_body_for_state(state: &str) -> Option<&'static str> {
    match state {
        "planning" => Some(PROMPT_PLANNING),
        "plan_review" => Some(PROMPT_PLAN_REVIEW),
        "implementation" => Some(PROMPT_IMPLEMENTATION),
        "implementation_review" => Some(PROMPT_IMPLEMENTATION_REVIEW),
        "shipment" => Some(PROMPT_SHIPMENT),
        "shipment_review" => Some(PROMPT_SHIPMENT_REVIEW),
        "evaluating" => Some(PROMPT_EVALUATING),
        _ => None,
    }
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
    fn builtin_loom_prompts_include_output_specific_delivery_targets() {
        assert!(PROMPT_IMPLEMENTATION.contains("`{{ output }}` = `remote_main`"));
        assert!(PROMPT_IMPLEMENTATION.contains("open or update the PR"));

        assert!(PROMPT_IMPLEMENTATION_REVIEW.contains("branch diff, status, and test results"));
        assert!(PROMPT_IMPLEMENTATION_REVIEW.contains("review the pull request itself"));

        assert!(PROMPT_SHIPMENT.contains("merge the feature branch to main"));
        assert!(PROMPT_SHIPMENT.contains("merge the approved pull request"));

        assert!(PROMPT_SHIPMENT_REVIEW.contains("review the code now on main"));
        assert!(PROMPT_SHIPMENT_REVIEW.contains("review the merged pull request"));
    }
}
