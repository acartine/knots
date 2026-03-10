const PLANNING: &str = include_str!("../skills/planning.md");
const PLAN_REVIEW: &str = include_str!("../skills/plan_review.md");
const EVALUATING: &str = include_str!("../skills/evaluating.md");
const IMPLEMENTATION: &str = include_str!("../skills/implementation.md");
const IMPLEMENTATION_REVIEW: &str = include_str!("../skills/implementation_review.md");
const SHIPMENT: &str = include_str!("../skills/shipment.md");
const SHIPMENT_REVIEW: &str = include_str!("../skills/shipment_review.md");

pub fn skill_for_state(state: &str) -> Option<&'static str> {
    match state {
        "planning" => Some(PLANNING),
        "plan_review" => Some(PLAN_REVIEW),
        "evaluating" => Some(EVALUATING),
        "implementation" => Some(IMPLEMENTATION),
        "implementation_review" => Some(IMPLEMENTATION_REVIEW),
        "shipment" => Some(SHIPMENT),
        "shipment_review" => Some(SHIPMENT_REVIEW),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::skill_for_state;

    fn assert_commit_tag_guidance(text: &str, state: &str) {
        assert!(
            text.contains(r#"--add-tag "commit:${short_hash}""#),
            "{} skill must include commit:${{short_hash}} tagging command",
            state
        );
        assert!(
            text.contains("git rev-parse --short=12 <commit>"),
            "{} skill must describe generating short hashes",
            state
        );
        assert!(
            text.contains("short hashes only"),
            "{} skill must require short hashes",
            state
        );
    }

    fn assert_review_write_constraints(text: &str, state: &str) {
        assert!(
            text.contains("read-only for repository code and git state"),
            "{} skill must declare review as read-only for code/git",
            state
        );
        assert!(
            text.contains("Do not edit code, tests, docs, configs"),
            "{} skill must prohibit code edits during review",
            state
        );
        assert!(
            text.contains("Do not run git write operations"),
            "{} skill must prohibit git write operations during review",
            state
        );
        assert!(
            text.contains("knot metadata updates only"),
            "{} skill must allow only knot metadata writes",
            state
        );
        assert!(
            text.contains("reject/failure path"),
            "{} skill must describe fallback when code/git writes are required",
            state
        );
    }

    fn assert_step_boundary(text: &str, state: &str) {
        let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            text.contains("## Step Boundary"),
            "{} skill must declare a step boundary section",
            state
        );
        assert!(
            text.contains(&format!("This session is authorized only for `{state}`.")),
            "{} skill must declare its single-step authority",
            state
        );
        assert!(
            text.contains("Complete exactly one"),
            "{} skill must require a single workflow action",
            state
        );
        assert!(
            normalized.contains("stop immediately"),
            "{} skill must include an immediate stop condition",
            state
        );
    }

    #[test]
    fn returns_content_for_action_states() {
        assert!(skill_for_state("planning").unwrap().contains("# Planning"));
        assert!(skill_for_state("implementation")
            .unwrap()
            .contains("# Implementation"));
        assert!(skill_for_state("evaluating")
            .unwrap()
            .contains("# Evaluating"));
        assert!(skill_for_state("shipment_review")
            .unwrap()
            .contains("# Shipment Review"));
    }

    #[test]
    fn returns_none_for_queue_and_terminal_states() {
        assert!(skill_for_state("ready_for_planning").is_none());
        assert!(skill_for_state("shipped").is_none());
        assert!(skill_for_state("abandoned").is_none());
    }

    #[test]
    fn implementation_skill_instructs_short_commit_tagging() {
        let text = skill_for_state("implementation").unwrap();
        assert_commit_tag_guidance(text, "implementation");
        assert!(
            text.contains("every commit created during implementation"),
            "implementation skill must require tagging every implementation commit"
        );
    }

    #[test]
    fn shipment_skill_instructs_short_commit_tagging() {
        let text = skill_for_state("shipment").unwrap();
        assert_commit_tag_guidance(text, "shipment");
        assert!(
            text.contains("each new commit created during shipment"),
            "shipment skill must require tagging each shipment commit"
        );
    }

    #[test]
    fn shipment_review_skill_validates_commit_tagging() {
        let text = skill_for_state("shipment_review").unwrap();
        assert!(text.contains("`commit:` prefix"));
        assert!(text.contains("git rev-parse --short=12 <commit>"));
        assert!(text.contains("not the full 40-character hash"));
    }

    #[test]
    fn shipment_review_skill_handles_dirty_workspace_failure_mode() {
        let text = skill_for_state("shipment_review").unwrap();
        assert!(text.contains("Unable to complete review due to dirty workspace"));
        assert!(text.contains("Roll status back to Ready For Impl before handoff."));
        assert!(text.contains("--status ready_for_implementation"));
        assert!(text.contains("--add-note \"<dirty workspace details>\""));
        assert!(text.contains("--add-handoff-capsule \"<dirty workspace handoff>\""));
    }

    #[test]
    fn review_skills_forbid_code_and_git_writes() {
        for state in ["plan_review", "implementation_review", "shipment_review"] {
            let text = skill_for_state(state).unwrap();
            assert_review_write_constraints(text, state);
        }
    }

    #[test]
    fn all_skills_define_single_step_boundaries() {
        for state in [
            "planning",
            "plan_review",
            "implementation",
            "implementation_review",
            "shipment",
            "shipment_review",
        ] {
            let text = skill_for_state(state).unwrap();
            assert_step_boundary(text, state);
        }
    }

    #[test]
    fn planning_skill_forbids_executing_child_knots() {
        let text = skill_for_state("planning").unwrap();
        assert!(text.contains("Creating child knots is planning output only."));
        assert!(text.contains("Do not claim, start, or"));
        assert!(text.contains("execute those child knots in this session."));
    }

    #[test]
    fn implementation_skill_does_not_include_shipment_work() {
        let text = skill_for_state("implementation").unwrap();
        assert!(text.contains("Do not merge the feature branch to main"));
        assert!(text.contains("Open or update a PR for the feature branch"));
        assert!(!text.contains("Merge the feature branch into main if the knot"));
    }

    #[test]
    fn implementation_review_skill_uses_only_code_and_spec_for_approval() {
        let text = skill_for_state("implementation_review").unwrap();
        assert!(
            text.contains("Base approval strictly on the code under review and the knot"),
            "implementation_review must scope approval to code and knot spec"
        );
        assert!(
            text.contains("acceptance criteria as the source of truth"),
            "implementation_review must prioritize acceptance criteria when present"
        );
        assert!(
            text.contains("Do not use knot notes or prior handoff_capsules"),
            "implementation_review must exclude notes and handoff_capsules from approval"
        );
        assert!(
            text.contains("specification and code drift"),
            "implementation_review must frame review around spec and code drift"
        );
    }

    #[test]
    fn implementation_review_rejections_require_enumerated_spec_violations() {
        let text = skill_for_state("implementation_review").unwrap();
        let required = "<enumerated violations of the";
        assert_eq!(
            text.matches(required).count(),
            3,
            "implementation_review must require enumerated violations in every rejection path"
        );
        assert!(
            text.contains("knot description and/or acceptance criteria"),
            "implementation_review rejection handoff must cite description and acceptance criteria"
        );
    }
}
