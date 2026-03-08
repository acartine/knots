use crate::app::KnotView;
use crate::domain::metadata::MetadataEntry;
use crate::knot_id::display_id;

pub fn render_prompt(knot: &KnotView, skill: &str, completion_cmd: &str) -> String {
    render_prompt_inner(knot, skill, completion_cmd, false)
}

pub fn render_prompt_verbose(
    knot: &KnotView,
    skill: &str,
    completion_cmd: &str,
    verbose: bool,
) -> String {
    render_prompt_inner(knot, skill, completion_cmd, verbose)
}

fn render_prompt_inner(
    knot: &KnotView,
    skill: &str,
    completion_cmd: &str,
    verbose: bool,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", knot.title));
    out.push_str(&render_header(knot));
    out.push('\n');
    if let Some(body) = knot.body.as_deref().filter(|b| !b.is_empty()) {
        out.push_str("## Description\n\n");
        out.push_str(body);
        out.push_str("\n\n");
    } else if let Some(desc) = knot.description.as_deref().filter(|d| !d.is_empty()) {
        out.push_str("## Description\n\n");
        out.push_str(desc);
        out.push_str("\n\n");
    }
    if !knot.invariants.is_empty() {
        out.push_str("## Invariants\n\n");
        for inv in &knot.invariants {
            out.push_str(&format!(
                "- **[{}]** {}\n",
                inv.invariant_type, inv.condition
            ));
        }
        out.push('\n');
    }
    if !knot.notes.is_empty() || !knot.handoff_capsules.is_empty() {
        out.push_str("## Notes\n\n");
        if verbose {
            for entry in &knot.notes {
                out.push_str(&format_entry(entry));
            }
            for entry in &knot.handoff_capsules {
                out.push_str(&format_entry(entry));
            }
        } else {
            if let Some(latest) = knot.notes.last() {
                out.push_str(&format_entry(latest));
            }
            if let Some(latest) = knot.handoff_capsules.last() {
                out.push_str(&format_entry(latest));
            }
        }
        if !verbose {
            let hint = crate::ui::hidden_metadata_hint(knot);
            if !hint.is_empty() {
                out.push('\n');
                out.push_str(&hint);
                out.push('\n');
            }
        }
        out.push('\n');
    }
    out.push_str("---\n\n");
    out.push_str(skill.trim_end());
    out.push_str("\n\n");
    out.push_str("## Completion\n\n");
    out.push_str(&format!("`{completion_cmd}`\n"));
    out
}

pub fn render_prompt_json(knot: &KnotView, skill: &str, completion_cmd: &str) -> serde_json::Value {
    render_prompt_json_verbose(knot, skill, completion_cmd, false)
}

pub fn render_prompt_json_verbose(
    knot: &KnotView,
    skill: &str,
    completion_cmd: &str,
    verbose: bool,
) -> serde_json::Value {
    let prompt_text = render_prompt_inner(knot, skill, completion_cmd, verbose);
    let mut json = serde_json::json!({
        "id": knot.id,
        "title": knot.title,
        "state": knot.state,
        "priority": knot.priority,
        "type": knot.knot_type.as_str(),
        "profile_id": knot.profile_id,
        "invariants": knot.invariants,
        "prompt": prompt_text,
    });
    if !verbose {
        let hint = crate::ui::hidden_metadata_hint(knot);
        if !hint.is_empty() {
            json.as_object_mut()
                .unwrap()
                .insert("other".to_string(), serde_json::Value::String(hint));
        }
    }
    json
}

fn render_header(knot: &KnotView) -> String {
    let sid = display_id(&knot.id);
    let prio = knot.priority.map_or("none".to_string(), |p| p.to_string());
    let knot_type = knot.knot_type.as_str();
    format!(
        "**ID**: {sid}  |  **Priority**: {prio}  |  **Type**: {knot_type}\n\
         **Profile**: {}  |  **State**: {}\n\n",
        knot.profile_id, knot.state,
    )
}

fn format_entry(entry: &MetadataEntry) -> String {
    let attribution = entry_attribution(entry);
    format!("- **[{attribution}]** {}\n", entry.content)
}

fn entry_attribution(entry: &MetadataEntry) -> String {
    let who = if entry.agentname != "unknown" {
        &entry.agentname
    } else {
        &entry.username
    };
    format!(
        "{} {}",
        who,
        &entry.datetime[..10.min(entry.datetime.len())]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::KnotView;
    use crate::domain::knot_type::KnotType;
    use crate::domain::metadata::MetadataEntry;

    fn sample_knot() -> KnotView {
        KnotView {
            id: "K-abc123".to_string(),
            alias: None,
            title: "Add poll command".to_string(),
            state: "ready_for_implementation".to_string(),
            updated_at: "2026-02-27T10:00:00Z".to_string(),
            body: Some("Implement kno poll and kno claim".to_string()),
            description: None,
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
            profile_id: "autopilot".to_string(),
            profile_etag: None,
            deferred_from_state: None,
            created_at: None,
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
}
