use crate::app::KnotView;

use super::{Palette, ShowField};

pub(super) fn format_knot_show(
    knot: &KnotView,
    palette: &Palette,
    vw: usize,
    verbose: bool,
) -> Vec<String> {
    let fields = knot_show_fields(knot, verbose);
    let mut lines = format_show_fields(&fields, palette, vw);
    if !verbose {
        let hint = hidden_metadata_hint(knot);
        if !hint.is_empty() {
            lines.push(String::new());
            lines.push(palette.dim(&hint));
        }
    }
    lines
}

pub(super) fn format_entry_inline(entry: &crate::domain::metadata::MetadataEntry) -> String {
    let who = if entry.agentname != "unknown" {
        &entry.agentname
    } else {
        &entry.username
    };
    format!(
        "[{} {}] {}",
        who,
        &entry.datetime[..10.min(entry.datetime.len())],
        entry.content
    )
}

pub(crate) fn hidden_metadata_hint(knot: &KnotView) -> String {
    let mut parts = Vec::new();
    if knot.notes.len() > 1 {
        parts.push(older_item_hint(knot.notes.len() - 1, "note", "notes"));
    }
    if knot.handoff_capsules.len() > 1 {
        parts.push(older_item_hint(
            knot.handoff_capsules.len() - 1,
            "handoff capsule",
            "handoff capsules",
        ));
    }
    if parts.is_empty() {
        return String::new();
    }
    format!(
        "{} not shown. Use -v/--verbose to see all.",
        parts.join(" and ")
    )
}

fn older_item_hint(count: usize, singular: &str, plural: &str) -> String {
    format!(
        "{count} older {}",
        if count == 1 { singular } else { plural }
    )
}

pub(super) fn knot_show_fields(knot: &KnotView, verbose: bool) -> Vec<ShowField> {
    let mut f = vec![ShowField::new("id", crate::knot_id::display_id(&knot.id))];
    if let Some(a) = knot.alias.as_deref() {
        f.push(ShowField::new("alias", crate::knot_id::display_alias(a)));
    }
    f.push(ShowField::new("title", knot.title.clone()));
    f.push(ShowField::new("state", knot.state.clone()));
    f.push(ShowField::new("updated_at", knot.updated_at.clone()));
    if let Some(v) = knot.created_at.as_deref() {
        f.push(ShowField::new("created_at", v));
    }
    if let Some(v) = knot.body.as_deref() {
        f.push(ShowField::new("body", v));
    }
    if let Some(v) = knot.description.as_deref() {
        f.push(ShowField::new("description", v));
    }
    if let Some(v) = knot.priority {
        f.push(ShowField::new("priority", v.to_string()));
    }
    f.push(ShowField::new("type", knot.knot_type.as_str()));
    f.push(ShowField::new("profile_id", knot.profile_id.clone()));
    if !knot.tags.is_empty() {
        f.push(ShowField::new("tags", knot.tags.join(", ")));
    }
    if !knot.verification_steps.is_empty() {
        f.push(ShowField::new(
            "verification_steps",
            numbered_list(&knot.verification_steps),
        ));
    }
    append_scope_fields(&mut f, knot);
    append_step_metadata_fields(&mut f, knot);
    append_metadata_fields(&mut f, knot, verbose);
    append_lease_agent_fields(&mut f, knot);
    append_gate_fields(&mut f, knot);
    append_edge_fields(&mut f, knot);
    f
}

fn append_metadata_fields(f: &mut Vec<ShowField>, knot: &KnotView, verbose: bool) {
    if !knot.notes.is_empty() {
        if verbose {
            for e in &knot.notes {
                f.push(ShowField::new("note", format_entry_inline(e)));
            }
        } else if let Some(l) = knot.notes.last() {
            f.push(ShowField::new("note", format_entry_inline(l)));
        }
    }
    if !knot.handoff_capsules.is_empty() {
        if verbose {
            for e in &knot.handoff_capsules {
                f.push(ShowField::new("handoff_capsule", format_entry_inline(e)));
            }
        } else if let Some(l) = knot.handoff_capsules.last() {
            f.push(ShowField::new("handoff_capsule", format_entry_inline(l)));
        }
    }
    if !knot.invariants.is_empty() {
        f.push(ShowField::new(
            "invariants",
            knot.invariants
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }
}

fn numbered_list(items: &[String]) -> String {
    items
        .iter()
        .enumerate()
        .map(|(idx, item)| format!("{}. {}", idx + 1, item))
        .collect::<Vec<_>>()
        .join("\n")
}

fn append_scope_fields(f: &mut Vec<ShowField>, knot: &KnotView) {
    let Some(scope) = knot.scope.as_ref() else {
        return;
    };
    if let Some(value) = scope.volume {
        f.push(ShowField::new("scope_volume", value.to_string()));
    }
    if let Some(value) = scope.scale.as_deref() {
        f.push(ShowField::new("scope_scale", value));
    }
    if let Some(value) = scope.volume_score_confidence {
        f.push(ShowField::new(
            "scope_volume_score_confidence",
            value.to_string(),
        ));
    }
    if let Some(value) = scope.volume_stddev {
        f.push(ShowField::new("scope_volume_stddev", value.to_string()));
    }
    if let Some(value) = scope.volume_result_id.as_deref() {
        f.push(ShowField::new("scope_volume_result_id", value));
    }
    if let Some(value) = scope.reliability {
        f.push(ShowField::new("scope_reliability", value.to_string()));
    }
    if let Some(value) = scope.reliability_score_confidence {
        f.push(ShowField::new(
            "scope_reliability_score_confidence",
            value.to_string(),
        ));
    }
    if let Some(value) = scope.reliability_stddev {
        f.push(ShowField::new(
            "scope_reliability_stddev",
            value.to_string(),
        ));
    }
    if let Some(value) = scope.reliability_band.as_deref() {
        f.push(ShowField::new("scope_reliability_band", value));
    }
    if let Some(value) = scope.reliability_result_id.as_deref() {
        f.push(ShowField::new("scope_reliability_result_id", value));
    }
}

fn append_step_metadata_fields(f: &mut Vec<ShowField>, knot: &KnotView) {
    if let Some(meta) = &knot.step_metadata {
        f.push(ShowField::new("step_owner", format_step_owner(meta)));
        if let Some(output) = &meta.output {
            f.push(ShowField::new("step_artifact", &output.artifact_type));
        }
        if let Some(hint) = &meta.review_hint {
            f.push(ShowField::new("step_review_hint", hint));
        }
    }
    if let Some(meta) = &knot.next_step_metadata {
        f.push(ShowField::new("next_owner", format_step_owner(meta)));
        if let Some(output) = &meta.output {
            f.push(ShowField::new("next_artifact", &output.artifact_type));
        }
        if let Some(hint) = &meta.review_hint {
            f.push(ShowField::new("next_review_hint", hint));
        }
    }
}

fn format_step_owner(meta: &crate::workflow::StepMetadata) -> String {
    match &meta.owner {
        Some(o) => match o.kind {
            crate::workflow::OwnerKind::Human => "human".to_string(),
            crate::workflow::OwnerKind::Agent => "agent".to_string(),
        },
        None => "unspecified".to_string(),
    }
}

fn append_gate_fields(f: &mut Vec<ShowField>, knot: &KnotView) {
    if let Some(g) = knot.gate.as_ref() {
        f.push(ShowField::new("gate_owner_kind", g.owner_kind.to_string()));
        if !g.failure_modes.is_empty() {
            f.push(ShowField::new(
                "gate_failure_modes",
                g.failure_modes
                    .iter()
                    .map(|(i, t)| format!("{i} => {}", t.join(", ")))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ));
        }
    }
}

fn append_lease_agent_fields(f: &mut Vec<ShowField>, knot: &KnotView) {
    if let Some(owner) = knot.lease.as_ref().and_then(|l| l.owner.as_ref()) {
        f.push(ShowField::new("lease_owner", owner.to_string()));
    }
    let Some(agent) = knot.lease_agent.as_ref() else {
        return;
    };
    f.push(ShowField::new(
        "lease_agent",
        format!(
            "agent_type={} provider={} agent_name={} model={} model_version={}",
            agent.agent_type, agent.provider, agent.agent_name, agent.model, agent.model_version
        ),
    ));
}

fn append_edge_fields(f: &mut Vec<ShowField>, knot: &KnotView) {
    if !knot.edges.is_empty() {
        for (kind, targets) in &group_edges_by_kind(&knot.edges, &knot.id) {
            f.push(ShowField::new(kind, targets.join(", ")));
        }
    }
}

fn group_edges_by_kind(
    edges: &[crate::app::EdgeView],
    knot_id: &str,
) -> Vec<(String, Vec<String>)> {
    use std::collections::BTreeMap;
    let mut g: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for e in edges {
        let (l, t) = if e.src == knot_id {
            (
                e.kind.clone(),
                crate::knot_id::display_id(&e.dst).to_string(),
            )
        } else {
            (
                format!("{} (incoming)", e.kind),
                crate::knot_id::display_id(&e.src).to_string(),
            )
        };
        g.entry(l).or_default().push(t);
    }
    g.into_iter().collect()
}

pub(super) fn format_show_fields(
    fields: &[ShowField],
    palette: &Palette,
    vw: usize,
) -> Vec<String> {
    if fields.is_empty() {
        return Vec::new();
    }
    let lw = fields.iter().map(|f| f.label.len() + 1).max().unwrap_or(0);
    let mut lines = Vec::new();
    for field in fields {
        let wrapped = wrap_value(&field.value, vw.max(1));
        let label = format!("{}:", field.label);
        for (i, chunk) in wrapped.iter().enumerate() {
            let lt = if i == 0 {
                format!("{label:>lw$}")
            } else {
                " ".repeat(lw)
            };
            lines.push(format!("{}  {}", palette.label(&lt), chunk));
        }
    }
    lines
}

pub(super) fn wrap_value(value: &str, width: usize) -> Vec<String> {
    if value.is_empty() {
        return vec![String::new()];
    }
    value
        .split('\n')
        .flat_map(|l| wrap_single_line(l, width))
        .collect()
}

fn wrap_single_line(line: &str, width: usize) -> Vec<String> {
    if line.is_empty() {
        return vec![String::new()];
    }
    let mut w = Vec::new();
    let mut r = line.trim_end_matches('\r');
    while char_count(r) > width {
        let si = wrap_split_index(r, width);
        w.push(r[..si].trim_end().to_string());
        r = r[si..].trim_start();
        if r.is_empty() {
            break;
        }
    }
    w.push(r.to_string());
    w
}

pub(super) fn wrap_split_index(text: &str, width: usize) -> usize {
    let mut lw = None;
    for (idx, ch, count) in indexed_chars(text) {
        if count > width {
            break;
        }
        if ch.is_whitespace() {
            lw = Some(idx);
        }
    }
    lw.unwrap_or_else(|| byte_index_at_char(text, width))
}

fn char_count(text: &str) -> usize {
    text.chars().count()
}

fn indexed_chars(text: &str) -> impl Iterator<Item = (usize, char, usize)> + '_ {
    text.char_indices()
        .enumerate()
        .map(|(pos, (idx, ch))| (idx, ch, pos + 1))
}

fn byte_index_at_char(text: &str, tc: usize) -> usize {
    text.char_indices()
        .nth(tc)
        .map_or(text.len(), |(idx, _)| idx)
}
