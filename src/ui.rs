use crate::app::KnotView;
use crate::doctor::{DoctorCheck, DoctorReport, DoctorStatus};
use crate::list_layout::DisplayKnot;
use crate::listing::KnotListFilter;
mod palette;
mod progress;
mod show_fields;
#[cfg(test)]
pub(crate) use palette::state_color_code;
pub(crate) use palette::Palette;
use palette::ShowField;
#[cfg(test)]
pub(crate) use progress::format_progress_line;
pub(crate) use progress::StdoutProgressReporter;
use show_fields::format_knot_show;
pub(crate) use show_fields::hidden_metadata_hint;
#[cfg(test)]
use show_fields::{
    format_entry_inline, format_show_fields, knot_show_fields, wrap_split_index, wrap_value,
};
const SHOW_VALUE_WIDTH: usize = 80;
pub fn trim_json_metadata(value: &mut serde_json::Value, knot: &KnotView) {
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
/// One `kno lease list` row. Surfaces the owning machine so a lease pulled
/// from another machine is distinguishable from one held here.
pub fn format_lease_row(lease: &KnotView, local_machine_id: &str, p: &Palette) -> String {
    let row = format!(
        "{} {} {}",
        p.id(&lease.id),
        p.state(&lease.state),
        lease.title
    );
    let Some(owner) = lease.lease.as_ref().and_then(|l| l.owner.as_ref()) else {
        return row;
    };
    let held = if owner.machine_id == local_machine_id {
        "this machine"
    } else {
        "another machine"
    };
    let short: String = owner.machine_id.chars().take(8).collect();
    format!(
        "{} {}",
        row,
        p.dim(&format!("(machine={} pid={} {})", short, owner.pid, held))
    )
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
        Some(a) => format!("{} ({sid})", crate::knot_id::display_alias(a)),
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
