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
            text.contains(r#"--add-tag "commit:<full-40-char-hash>""#),
            "{} skill must include commit:<full-40-char-hash> tagging command",
            state
        );
        assert!(
            text.contains("full 40-character hash"),
            "{} skill must require full 40-character hashes",
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
    fn implementation_skill_instructs_full_commit_tagging() {
        let text = skill_for_state("implementation").unwrap();
        assert_commit_tag_guidance(text, "implementation");
        assert!(
            text.contains("every commit created during implementation"),
            "implementation skill must require tagging every implementation commit"
        );
    }

    #[test]
    fn shipment_skill_instructs_full_commit_tagging() {
        let text = skill_for_state("shipment").unwrap();
        assert_commit_tag_guidance(text, "shipment");
        assert!(
            text.contains("each new commit created during shipment"),
            "shipment skill must require tagging each shipment commit"
        );
    }
}
