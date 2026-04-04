use crate::app::KnotView;
use crate::domain::knot_type::KnotType;
use crate::domain::metadata::MetadataEntry;
use crate::prompt::{
    render_prompt, render_prompt_json, render_prompt_json_verbose, render_prompt_verbose,
};

fn sample_knot() -> KnotView {
    KnotView {
        id: "K-abc123".to_string(),
        alias: None,
        title: "Add poll command".to_string(),
        state: "ready_for_implementation".to_string(),
        updated_at: "2026-02-27T10:00:00Z".to_string(),
        body: Some("Implement kno poll and kno claim".to_string()),
        description: None,
        acceptance: None,
        priority: Some(1),
        knot_type: KnotType::default(),
        tags: vec![],
        notes: vec![MetadataEntry {
            entry_id: "e1".to_string(),
            content: "Plan approved".to_string(),
            username: "alice".to_string(),
            datetime: "2026-02-27T09:00:00Z".to_string(),
            agentname: "unknown".to_string(),
            model: "unknown".to_string(),
            version: "unknown".to_string(),
        }],
        handoff_capsules: vec![],
        invariants: vec![],
        step_history: vec![],
        gate: None,
        lease: None,
        lease_id: None,
        lease_expiry_ts: 0,
        lease_agent: None,
        workflow_id: "compatibility".to_string(),
        profile_id: "autopilot".to_string(),
        profile_etag: None,
        deferred_from_state: None,
        blocked_from_state: None,
        created_at: None,
        step_metadata: None,
        next_step_metadata: None,
        edges: vec![],
        child_summaries: vec![],
    }
}

fn make_entry(id: &str, content: &str, agent: &str) -> MetadataEntry {
    MetadataEntry {
        entry_id: id.to_string(),
        content: content.to_string(),
        username: "user".to_string(),
        datetime: "2026-03-01T10:00:00Z".to_string(),
        agentname: agent.to_string(),
        model: "m".to_string(),
        version: "v".to_string(),
    }
}

#[test]
fn render_contains_title_and_id() {
    let knot = sample_knot();
    let output = render_prompt(&knot, "# Implementation\n", "kno state K-abc123 done");
    assert!(output.contains("# Add poll command"));
    assert!(output.contains("abc123"));
}

#[test]
fn render_contains_skill_and_completion() {
    let knot = sample_knot();
    let cmd = "kno state K-abc123 ready_for_implementation_review";
    let output = render_prompt(&knot, "# Implementation\nDo the work.\n", cmd);
    assert!(output.contains("## Workflow Boundary"));
    assert!(output.contains("Complete exactly one workflow action, then stop."));
    assert!(output.contains("Do not claim or execute another knot"));
    assert!(output.contains("# Implementation"));
    assert!(output.contains("Do the work."));
    assert!(output.contains("## Completion"));
    assert!(output.contains(cmd));
}

#[test]
fn render_includes_notes() {
    let knot = sample_knot();
    let output = render_prompt(&knot, "# Skill\n", "kno state x y");
    assert!(output.contains("Plan approved"));
    assert!(output.contains("alice"));
}

#[test]
fn render_uses_body_over_description() {
    let mut knot = sample_knot();
    knot.description = Some("short desc".to_string());
    let output = render_prompt(&knot, "# S\n", "cmd");
    assert!(output.contains("Implement kno poll"));
    assert!(!output.contains("short desc"));
}

#[test]
fn render_falls_back_to_description() {
    let mut knot = sample_knot();
    knot.body = None;
    knot.description = Some("short desc".to_string());
    let output = render_prompt(&knot, "# S\n", "cmd");
    assert!(output.contains("short desc"));
}

#[test]
fn render_includes_acceptance_section() {
    let mut knot = sample_knot();
    knot.acceptance = Some("Must preserve round-trip reads.".to_string());
    let output = render_prompt(&knot, "# S\n", "cmd");
    assert!(output.contains("## Acceptance Criteria"));
    assert!(output.contains("Must preserve round-trip reads."));
}

#[test]
fn render_includes_invariants() {
    use crate::domain::invariant::{Invariant, InvariantType};
    let mut knot = sample_knot();
    knot.invariants = vec![
        Invariant::new(InvariantType::Scope, "only touch src/prompt.rs").unwrap(),
        Invariant::new(InvariantType::State, "tests must pass").unwrap(),
    ];
    let output = render_prompt(&knot, "# S\n", "cmd");
    assert!(output.contains("## Invariants"));
    assert!(output.contains("**[Scope]** only touch src/prompt.rs"));
    assert!(output.contains("**[State]** tests must pass"));
}

#[test]
fn render_omits_invariants_section_when_empty() {
    let knot = sample_knot();
    let output = render_prompt(&knot, "# S\n", "cmd");
    assert!(!output.contains("## Invariants"));
}

#[test]
fn render_no_body_or_description_omits_section() {
    let mut knot = sample_knot();
    knot.body = None;
    knot.description = None;
    let output = render_prompt(&knot, "# S\n", "cmd");
    assert!(!output.contains("## Description"));
}

#[test]
fn render_empty_body_falls_back_to_description() {
    let mut knot = sample_knot();
    knot.body = Some(String::new());
    knot.description = Some("fallback desc".to_string());
    let output = render_prompt(&knot, "# S\n", "cmd");
    assert!(output.contains("fallback desc"));
}

#[test]
fn render_handoff_capsules_appear_in_notes() {
    let mut knot = sample_knot();
    knot.handoff_capsules = vec![MetadataEntry {
        entry_id: "h1".to_string(),
        content: "handoff content".to_string(),
        username: "bob".to_string(),
        datetime: "2026-02-28T09:00:00Z".to_string(),
        agentname: "agent1".to_string(),
        model: "m".to_string(),
        version: "v".to_string(),
    }];
    let output = render_prompt(&knot, "# S\n", "cmd");
    assert!(output.contains("handoff content"));
    assert!(output.contains("agent1"));
}

#[test]
fn render_no_priority_shows_none() {
    let mut knot = sample_knot();
    knot.priority = None;
    let output = render_prompt(&knot, "# S\n", "cmd");
    assert!(output.contains("**Priority**: none"));
}

#[test]
fn json_output_includes_invariants() {
    use crate::domain::invariant::{Invariant, InvariantType};
    let mut knot = sample_knot();
    knot.invariants = vec![Invariant::new(InvariantType::Scope, "limit scope").unwrap()];
    let json = render_prompt_json(&knot, "# Skill\n", "kno state x y");
    let inv_arr = json["invariants"].as_array().unwrap();
    assert_eq!(inv_arr.len(), 1);
    assert_eq!(inv_arr[0]["type"], "Scope");
}

#[test]
fn json_output_has_expected_fields() {
    let knot = sample_knot();
    let json = render_prompt_json(&knot, "# Skill\n", "kno state x y");
    assert_eq!(json["id"], "K-abc123");
    assert_eq!(json["title"], "Add poll command");
    assert!(json["prompt"]
        .as_str()
        .unwrap()
        .contains("# Add poll command"));
}

#[test]
fn render_non_verbose_shows_only_latest_note() {
    let mut knot = sample_knot();
    knot.notes = vec![
        make_entry("n1", "old note", "agent1"),
        make_entry("n2", "new note", "agent2"),
    ];
    let output = render_prompt_verbose(&knot, "# S\n", "cmd", false);
    assert!(!output.contains("old note"));
    assert!(output.contains("new note"));
    assert!(output.contains("1 older note"));
}

#[test]
fn render_verbose_shows_all_notes() {
    let mut knot = sample_knot();
    knot.notes = vec![
        make_entry("n1", "old note", "agent1"),
        make_entry("n2", "new note", "agent2"),
    ];
    let output = render_prompt_verbose(&knot, "# S\n", "cmd", true);
    assert!(output.contains("old note"));
    assert!(output.contains("new note"));
    assert!(!output.contains("not shown"));
}

#[test]
fn render_non_verbose_shows_latest_handoff() {
    let mut knot = sample_knot();
    knot.handoff_capsules = vec![
        make_entry("h1", "old handoff", "a1"),
        make_entry("h2", "new handoff", "a2"),
    ];
    let output = render_prompt_verbose(&knot, "# S\n", "cmd", false);
    assert!(!output.contains("old handoff"));
    assert!(output.contains("new handoff"));
}

#[test]
fn json_verbose_omits_other_field() {
    let mut knot = sample_knot();
    knot.notes = vec![make_entry("n1", "old", "a"), make_entry("n2", "new", "a")];
    let json = render_prompt_json_verbose(&knot, "# S\n", "cmd", true);
    assert!(json.get("other").is_none());
}

#[test]
fn json_non_verbose_includes_other_field() {
    let mut knot = sample_knot();
    knot.notes = vec![make_entry("n1", "old", "a"), make_entry("n2", "new", "a")];
    let json = render_prompt_json_verbose(&knot, "# S\n", "cmd", false);
    let other = json["other"].as_str().unwrap();
    assert!(other.contains("1 older note"));
}

#[test]
fn json_no_other_when_single_entries() {
    let knot = sample_knot();
    let json = render_prompt_json_verbose(&knot, "# S\n", "cmd", false);
    assert!(json.get("other").is_none());
}

#[test]
fn render_children_section_when_children_present() {
    use crate::app::ChildSummary;
    let mut knot = sample_knot();
    knot.child_summaries = vec![
        ChildSummary {
            id: "K-child1".to_string(),
            title: "First child".to_string(),
            state: "ready_for_planning".to_string(),
        },
        ChildSummary {
            id: "K-child2".to_string(),
            title: "Second child".to_string(),
            state: "planning".to_string(),
        },
    ];
    let output = render_prompt(&knot, "# S\n", "cmd");
    assert!(output.contains("## Children"));
    assert!(output.contains("First child"));
    assert!(output.contains("Second child"));
    assert!(output.contains("kno claim <child-id>"));
}

#[test]
fn render_omits_children_section_when_empty() {
    let knot = sample_knot();
    let output = render_prompt(&knot, "# S\n", "cmd");
    assert!(!output.contains("## Children"));
}

#[test]
fn workflow_boundary_allows_child_claims_for_parents() {
    use crate::app::ChildSummary;
    let mut knot = sample_knot();
    knot.child_summaries = vec![ChildSummary {
        id: "K-child1".to_string(),
        title: "Child".to_string(),
        state: "ready_for_planning".to_string(),
    }];
    let output = render_prompt(&knot, "# S\n", "cmd");
    assert!(output.contains("You may claim the child knots listed above"));
    assert!(!output.contains("Do not claim or execute another knot"));
}

#[test]
fn workflow_boundary_restricts_claims_without_children() {
    let knot = sample_knot();
    let output = render_prompt(&knot, "# S\n", "cmd");
    assert!(output.contains("Do not claim or execute another knot"));
    assert!(!output.contains("You may claim the child knots listed above"));
}

#[test]
fn json_output_includes_child_summaries() {
    use crate::app::ChildSummary;
    let mut knot = sample_knot();
    knot.child_summaries = vec![ChildSummary {
        id: "K-child1".to_string(),
        title: "Child".to_string(),
        state: "planning".to_string(),
    }];
    let json = render_prompt_json(&knot, "# S\n", "cmd");
    let children = json["child_summaries"].as_array().unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0]["id"], "K-child1");
    assert_eq!(children[0]["state"], "planning");
}
