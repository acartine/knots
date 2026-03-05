const PLANNING: &str = include_str!("../skills/planning.md");
const PLAN_REVIEW: &str = include_str!("../skills/plan_review.md");
const IMPLEMENTATION: &str = include_str!("../skills/implementation.md");
const IMPLEMENTATION_REVIEW: &str = include_str!("../skills/implementation_review.md");
const SHIPMENT: &str = include_str!("../skills/shipment.md");
const SHIPMENT_REVIEW: &str = include_str!("../skills/shipment_review.md");

pub fn skill_for_state(state: &str) -> Option<&'static str> {
    match state {
        "planning" => Some(PLANNING),
        "plan_review" => Some(PLAN_REVIEW),
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

    #[test]
    fn returns_content_for_action_states() {
        assert!(skill_for_state("planning").unwrap().contains("# Planning"));
        assert!(skill_for_state("implementation")
            .unwrap()
            .contains("# Implementation"));
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
    fn review_skills_forbid_code_and_git_writes() {
        for state in ["plan_review", "implementation_review", "shipment_review"] {
            let text = skill_for_state(state).unwrap();
            assert_review_write_constraints(text, state);
        }
    }
}
