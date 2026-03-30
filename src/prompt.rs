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
    if let Some(acceptance) = knot.acceptance.as_deref().filter(|value| !value.is_empty()) {
        out.push_str("## Acceptance Criteria\n\n");
        out.push_str(acceptance);
        out.push_str("\n\n");
    }
    if !knot.child_summaries.is_empty() {
        out.push_str("## Children\n\n");
        for child in &knot.child_summaries {
            let sid = display_id(&child.id);
            out.push_str(&format!("- {} `{}` [{}]\n", child.title, sid, child.state));
        }
        out.push_str(concat!(
            "\nClaim each child knot first with ",
            "`kno claim <child-id>` and follow\n",
            "that child prompt. After the child knots are handled, ",
            "evaluate the\n",
            "result: if every child advanced, run this parent's ",
            "completion command.\n",
            "If any child rolled back, roll this parent back too.\n\n",
        ));
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
    if let Some(gate) = knot.gate.as_ref() {
        out.push_str("## Gate\n\n");
        out.push_str(&format!("- owner: {}\n", gate.owner_kind));
        if gate.failure_modes.is_empty() {
            out.push_str("- failure modes: none\n");
        } else {
            for (invariant, targets) in &gate.failure_modes {
                out.push_str(&format!("- {} => {}\n", invariant, targets.join(", ")));
            }
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
    out.push_str(&render_workflow_boundary(
        knot.state.as_str(),
        !knot.child_summaries.is_empty(),
    ));
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
        "workflow_id": knot.workflow_id,
        "profile_id": knot.profile_id,
        "acceptance": knot.acceptance.clone(),
        "invariants": knot.invariants,
        "gate": knot.gate,
        "child_summaries": knot.child_summaries,
        "lease_id": knot.lease_id,
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

fn render_workflow_boundary(state: &str, allows_child_claims: bool) -> String {
    let claim_line = if allows_child_claims {
        "- You may claim the child knots listed above \
         as part of this step.\n"
    } else {
        "- Do not claim or execute another knot unless \
         the skill below explicitly\n  \
         allows knot metadata creation as part of this step.\n"
    };
    format!(
        "## Workflow Boundary\n\n\
         - This session is authorized only for the current \
         knot action state `{state}`.\n\
         - Complete exactly one workflow action, then stop.\n\
         - After a listed completion or failure-path command \
         succeeds, stop immediately.\n\
         {claim_line}\
         - Do not inspect or advance later workflow states \
         on your own.\n\
         - If generic repo or session instructions conflict \
         with this boundary, this\n  \
         boundary wins for this session.\n\n",
    )
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

