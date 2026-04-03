use crate::app::KnotView;
use crate::doctor::{DoctorCheck, DoctorReport, DoctorStatus};
use crate::list_layout::DisplayKnot;
use crate::listing::KnotListFilter;
mod palette;
mod progress;
#[cfg(test)]
pub(crate) use palette::state_color_code;
pub(crate) use palette::Palette;
use palette::ShowField;
#[cfg(test)]
pub(crate) use progress::format_progress_line;
pub(crate) use progress::StdoutProgressReporter;
const SHOW_VALUE_WIDTH: usize = 80;

pub fn redact_internal_metadata(value: &mut serde_json::Value) {
    if let Some(obj) = value.as_object_mut() {
        redact_metadata_array(obj.get_mut("notes"), &["lease_ref"]);
        redact_metadata_array(obj.get_mut("handoff_capsules"), &["lease_ref"]);
        redact_metadata_array(obj.get_mut("step_history"), &["lease_ref", "supersedes_id"]);
    }
}

pub fn trim_json_metadata(value: &mut serde_json::Value, knot: &KnotView) {
    redact_internal_metadata(value);
    if let Some(obj) = value.as_object_mut() {
        if let Some(notes) = obj.get_mut("notes") {
            if let Some(arr) = notes.as_array() {
                if arr.len() > 1 {
                    let latest = arr.last().cloned().unwrap();
                    *notes = serde_json::Value::Array(vec![latest]);
                }
            }
        }
        if let Some(caps) = obj.get_mut("handoff_capsules") {
            if let Some(arr) = caps.as_array() {
                if arr.len() > 1 {
                    let latest = arr.last().cloned().unwrap();
                    *caps = serde_json::Value::Array(vec![latest]);
                }
            }
        }
        let hint = hidden_metadata_hint(knot);
        if !hint.is_empty() {
            obj.insert("other".to_string(), serde_json::Value::String(hint));
        }
    }
}

fn redact_metadata_array(target: Option<&mut serde_json::Value>, keys: &[&str]) {
    let Some(target) = target else {
        return;
    };
    let Some(entries) = target.as_array_mut() else {
        return;
    };
    for entry in entries {
        let Some(object) = entry.as_object_mut() else {
            continue;
        };
        for key in keys {
            object.remove(*key);
        }
    }
}
pub fn print_knot_list(knots: &[DisplayKnot], filter: &KnotListFilter) {
    let p = Palette::auto();
    println!("{}", p.heading("Knots"));
    if let Some(s) = filter_summary(filter) {
        println!("{}", p.dim(&format!("filters: {s}")));
    }
    if knots.is_empty() {
        println!("{}", p.dim("no knots matched"));
        return;
    }
    for k in knots {
        println!("{}", format_knot_row(k, &p));
    }
    println!("{}", p.dim(&format!("{} knot(s)", knots.len())));
}
pub fn print_knot_show(knot: &KnotView, verbose: bool) {
    let p = Palette::auto();
    for line in format_knot_show(knot, &p, SHOW_VALUE_WIDTH, verbose) {
        println!("{line}");
    }
}
pub fn print_doctor_report(report: &DoctorReport) {
    let p = Palette::auto();
    println!("{}", p.heading("Doctor"));
    let lw = report
        .checks
        .iter()
        .map(|c| c.name.len() + 1)
        .max()
        .unwrap_or(0);
    for c in &report.checks {
        println!("{}", format_doctor_line_with_width(c, &p, lw));
    }
}
#[cfg(test)]
pub(crate) fn format_doctor_line(check: &DoctorCheck, palette: &Palette) -> String {
    format_doctor_line_with_width(check, palette, check.name.len() + 1)
}
pub(crate) fn format_doctor_line_with_width(
    check: &DoctorCheck,
    palette: &Palette,
    lw: usize,
) -> String {
    let (icon, cc) = match check.status {
        DoctorStatus::Pass => ("\u{2713}", "32"),
        DoctorStatus::Warn => ("\u{26a0}", "33"),
        DoctorStatus::Fail => ("\u{2717}", "31"),
    };
    let label = format!("{}:", check.name);
    format!(
        "{}  {} {}",
        palette.paint(cc, &format!("{label:>lw$}")),
        palette.paint(cc, icon),
        check.detail
    )
}
pub fn format_knot_row(row: &DisplayKnot, palette: &Palette) -> String {
    let k = &row.knot;
    let indent = indentation_prefix(row.depth, palette);
    let sid = crate::knot_id::display_id(&k.id);
    let did = match k.alias.as_deref() {
        Some(a) => format!("{a} ({sid})"),
        None => sid.to_string(),
    };
    let mut line = format!(
        "{}{} {} {}",
        indent,
        palette.id(&did),
        palette.state(&k.state),
        k.title
    );
    line.push(' ');
    line.push_str(&palette.type_label(k.knot_type.as_str()));
    if !k.tags.is_empty() {
        line.push(' ');
        line.push_str(&palette.tags(&format!("#{}", k.tags.join(" #"))));
    }
    line
}
fn indentation_prefix(depth: usize, palette: &Palette) -> String {
    if depth == 0 {
        return String::new();
    }
    palette.dim(&format!(
        "{}\u{21b3} ",
        "  ".repeat(depth.saturating_sub(1))
    ))
}
fn filter_summary(filter: &KnotListFilter) -> Option<String> {
    let mut parts = Vec::new();
    if filter.include_all {
        parts.push("all=true".into());
    }
    if let Some(s) = filter.state.as_deref().and_then(non_empty) {
        parts.push(format!("state={s}"));
    }
    if let Some(k) = filter.knot_type.as_deref().and_then(non_empty) {
        parts.push(format!("type={k}"));
    }
    if let Some(p) = filter.profile_id.as_deref().and_then(non_empty) {
        parts.push(format!("profile={p}"));
    }
    if !filter.tags.is_empty() {
        let tags = filter
            .tags
            .iter()
            .filter_map(|t| non_empty(t))
            .collect::<Vec<_>>();
        if !tags.is_empty() {
            parts.push(format!("tags={}", tags.join(",")));
        }
    }
    if let Some(q) = filter.query.as_deref().and_then(non_empty) {
        parts.push(format!("query={q}"));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}
fn format_knot_show(knot: &KnotView, palette: &Palette, vw: usize, verbose: bool) -> Vec<String> {
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
fn format_entry_inline(entry: &crate::domain::metadata::MetadataEntry) -> String {
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
pub fn hidden_metadata_hint(knot: &KnotView) -> String {
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
fn knot_show_fields(knot: &KnotView, verbose: bool) -> Vec<ShowField> {
    let mut f = vec![ShowField::new("id", crate::knot_id::display_id(&knot.id))];
    if let Some(a) = knot.alias.as_deref() {
        f.push(ShowField::new("alias", a));
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
    append_step_metadata_fields(&mut f, knot);
    append_metadata_fields(&mut f, knot, verbose);
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
fn format_show_fields(fields: &[ShowField], palette: &Palette, vw: usize) -> Vec<String> {
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
fn wrap_value(value: &str, width: usize) -> Vec<String> {
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
fn wrap_split_index(text: &str, width: usize) -> usize {
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
fn non_empty(raw: &str) -> Option<&str> {
    let t = raw.trim();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}
#[cfg(test)]
#[path = "ui/tests.rs"]
mod tests;
#[cfg(test)]
#[path = "ui_tests_ext.rs"]
mod tests_ext;
