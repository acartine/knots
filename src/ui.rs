use std::io::{self, IsTerminal};

use crate::app::KnotView;
use crate::doctor::{DoctorCheck, DoctorReport, DoctorStatus};
use crate::list_layout::DisplayKnot;
use crate::listing::KnotListFilter;

mod progress;

#[cfg(test)]
pub(crate) use progress::format_progress_line;
pub(crate) use progress::StdoutProgressReporter;

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

pub fn print_knot_list(knots: &[DisplayKnot], filter: &KnotListFilter) {
    let palette = Palette::auto();
    println!("{}", palette.heading("Knots"));
    if let Some(summary) = filter_summary(filter) {
        println!("{}", palette.dim(&format!("filters: {summary}")));
    }

    if knots.is_empty() {
        println!("{}", palette.dim("no knots matched"));
        return;
    }

    for knot in knots {
        println!("{}", format_knot_row(knot, &palette));
    }
    println!("{}", palette.dim(&format!("{} knot(s)", knots.len())));
}

pub fn print_knot_show(knot: &KnotView, verbose: bool) {
    let palette = Palette::auto();
    for line in format_knot_show(knot, &palette, SHOW_VALUE_WIDTH, verbose) {
        println!("{line}");
    }
}

pub fn print_doctor_report(report: &DoctorReport) {
    let palette = Palette::auto();
    println!("{}", palette.heading("Doctor"));
    let label_width = report
        .checks
        .iter()
        .map(|check| check.name.len() + 1)
        .max()
        .unwrap_or(0);
    for check in &report.checks {
        println!(
            "{}",
            format_doctor_line_with_width(check, &palette, label_width)
        );
    }
}

#[cfg(test)]
pub(crate) fn format_doctor_line(check: &DoctorCheck, palette: &Palette) -> String {
    format_doctor_line_with_width(check, palette, check.name.len() + 1)
}

pub(crate) fn format_doctor_line_with_width(
    check: &DoctorCheck,
    palette: &Palette,
    label_width: usize,
) -> String {
    let (icon, color_code) = match check.status {
        DoctorStatus::Pass => ("\u{2713}", "32"),
        DoctorStatus::Warn => ("\u{26a0}", "33"),
        DoctorStatus::Fail => ("\u{2717}", "31"),
    };
    let label = format!("{}:", check.name);
    format!(
        "{}  {} {}",
        palette.paint(color_code, &format!("{label:>label_width$}")),
        palette.paint(color_code, icon),
        check.detail
    )
}

pub fn format_knot_row(row: &DisplayKnot, palette: &Palette) -> String {
    let knot = &row.knot;
    let indent = indentation_prefix(row.depth, palette);
    let short_id = crate::knot_id::display_id(&knot.id);
    let display_id = match knot.alias.as_deref() {
        Some(alias) => format!("{alias} ({short_id})"),
        None => short_id.to_string(),
    };
    let mut line = format!(
        "{}{} {} {}",
        indent,
        palette.id(&display_id),
        palette.state(&knot.state),
        knot.title
    );

    line.push(' ');
    line.push_str(&palette.type_label(knot.knot_type.as_str()));

    if !knot.tags.is_empty() {
        line.push(' ');
        line.push_str(&palette.tags(&format!("#{}", knot.tags.join(" #"))));
    }

    line
}

fn indentation_prefix(depth: usize, palette: &Palette) -> String {
    if depth == 0 {
        return String::new();
    }
    let spaces = "  ".repeat(depth.saturating_sub(1));
    palette.dim(&format!("{spaces}↳ "))
}

fn filter_summary(filter: &KnotListFilter) -> Option<String> {
    let mut parts = Vec::new();
    if filter.include_all {
        parts.push("all=true".to_string());
    }
    if let Some(state) = filter.state.as_deref().and_then(non_empty) {
        parts.push(format!("state={state}"));
    }
    if let Some(kind) = filter.knot_type.as_deref().and_then(non_empty) {
        parts.push(format!("type={kind}"));
    }
    if let Some(profile_id) = filter.profile_id.as_deref().and_then(non_empty) {
        parts.push(format!("profile={profile_id}"));
    }
    if !filter.tags.is_empty() {
        let tags = filter
            .tags
            .iter()
            .filter_map(|tag| non_empty(tag))
            .collect::<Vec<_>>();
        if !tags.is_empty() {
            parts.push(format!("tags={}", tags.join(",")));
        }
    }
    if let Some(query) = filter.query.as_deref().and_then(non_empty) {
        parts.push(format!("query={query}"));
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

fn format_knot_show(
    knot: &KnotView,
    palette: &Palette,
    value_width: usize,
    verbose: bool,
) -> Vec<String> {
    let fields = knot_show_fields(knot, verbose);
    let mut lines = format_show_fields(&fields, palette, value_width);
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
    let date = &entry.datetime[..10.min(entry.datetime.len())];
    format!("[{who} {date}] {}", entry.content)
}

pub fn hidden_metadata_hint(knot: &KnotView) -> String {
    let mut parts = Vec::new();
    if knot.notes.len() > 1 {
        let n = knot.notes.len() - 1;
        parts.push(older_item_hint(n, "note", "notes"));
    }
    if knot.handoff_capsules.len() > 1 {
        let n = knot.handoff_capsules.len() - 1;
        parts.push(older_item_hint(n, "handoff capsule", "handoff capsules"));
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
    let label = if count == 1 { singular } else { plural };
    format!("{count} older {label}")
}

fn knot_show_fields(knot: &KnotView, verbose: bool) -> Vec<ShowField> {
    let mut fields = vec![ShowField::new("id", crate::knot_id::display_id(&knot.id))];
    if let Some(alias) = knot.alias.as_deref() {
        fields.push(ShowField::new("alias", alias));
    }
    fields.push(ShowField::new("title", knot.title.clone()));
    fields.push(ShowField::new("state", knot.state.clone()));
    fields.push(ShowField::new("updated_at", knot.updated_at.clone()));
    if let Some(created_at) = knot.created_at.as_deref() {
        fields.push(ShowField::new("created_at", created_at));
    }
    if let Some(body) = knot.body.as_deref() {
        fields.push(ShowField::new("body", body));
    }
    if let Some(description) = knot.description.as_deref() {
        fields.push(ShowField::new("description", description));
    }
    if let Some(priority) = knot.priority {
        fields.push(ShowField::new("priority", priority.to_string()));
    }
    fields.push(ShowField::new("type", knot.knot_type.as_str()));
    fields.push(ShowField::new("profile_id", knot.profile_id.clone()));
    if !knot.tags.is_empty() {
        fields.push(ShowField::new("tags", knot.tags.join(", ")));
    }
    if !knot.notes.is_empty() {
        if verbose {
            for entry in &knot.notes {
                fields.push(ShowField::new("note", format_entry_inline(entry)));
            }
        } else if let Some(latest) = knot.notes.last() {
            fields.push(ShowField::new("note", format_entry_inline(latest)));
        }
    }
    if !knot.handoff_capsules.is_empty() {
        if verbose {
            for entry in &knot.handoff_capsules {
                fields.push(ShowField::new(
                    "handoff_capsule",
                    format_entry_inline(entry),
                ));
            }
        } else if let Some(latest) = knot.handoff_capsules.last() {
            fields.push(ShowField::new(
                "handoff_capsule",
                format_entry_inline(latest),
            ));
        }
    }
    if !knot.invariants.is_empty() {
        let formatted = knot
            .invariants
            .iter()
            .map(|inv| inv.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        fields.push(ShowField::new("invariants", formatted));
    }
    if let Some(gate) = knot.gate.as_ref() {
        fields.push(ShowField::new(
            "gate_owner_kind",
            gate.owner_kind.to_string(),
        ));
        if !gate.failure_modes.is_empty() {
            let formatted = gate
                .failure_modes
                .iter()
                .map(|(invariant, targets)| format!("{invariant} => {}", targets.join(", ")))
                .collect::<Vec<_>>()
                .join("\n");
            fields.push(ShowField::new("gate_failure_modes", formatted));
        }
    }
    if !knot.edges.is_empty() {
        let grouped = group_edges_by_kind(&knot.edges, &knot.id);
        for (kind, targets) in &grouped {
            let value = targets.join(", ");
            fields.push(ShowField::new(kind, value));
        }
    }
    fields
}

fn group_edges_by_kind(
    edges: &[crate::app::EdgeView],
    knot_id: &str,
) -> Vec<(String, Vec<String>)> {
    use std::collections::BTreeMap;
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for edge in edges {
        let (label, target) = if edge.src == knot_id {
            (
                edge.kind.clone(),
                crate::knot_id::display_id(&edge.dst).to_string(),
            )
        } else {
            let label = format!("{} (incoming)", edge.kind);
            (label, crate::knot_id::display_id(&edge.src).to_string())
        };
        groups.entry(label).or_default().push(target);
    }
    groups.into_iter().collect()
}

fn format_show_fields(fields: &[ShowField], palette: &Palette, value_width: usize) -> Vec<String> {
    if fields.is_empty() {
        return Vec::new();
    }
    let label_width = fields
        .iter()
        .map(|field| field.label.len() + 1)
        .max()
        .unwrap_or(0);
    let mut lines = Vec::new();
    for field in fields {
        let wrapped = wrap_value(&field.value, value_width.max(1));
        let label = format!("{}:", field.label);
        for (idx, chunk) in wrapped.iter().enumerate() {
            let label_text = if idx == 0 {
                format!("{label:>label_width$}")
            } else {
                " ".repeat(label_width)
            };
            lines.push(format!("{}  {}", palette.label(&label_text), chunk));
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
        .flat_map(|line| wrap_single_line(line, width))
        .collect()
}

fn wrap_single_line(line: &str, width: usize) -> Vec<String> {
    if line.is_empty() {
        return vec![String::new()];
    }
    let mut wrapped = Vec::new();
    let mut remaining = line.trim_end_matches('\r');
    while char_count(remaining) > width {
        let split_idx = wrap_split_index(remaining, width);
        let chunk = remaining[..split_idx].trim_end();
        wrapped.push(chunk.to_string());
        remaining = remaining[split_idx..].trim_start();
        if remaining.is_empty() {
            break;
        }
    }
    wrapped.push(remaining.to_string());
    wrapped
}

fn wrap_split_index(text: &str, width: usize) -> usize {
    let mut last_whitespace = None;
    for (idx, ch, count) in indexed_chars(text) {
        if count > width {
            break;
        }
        if ch.is_whitespace() {
            last_whitespace = Some(idx);
        }
    }
    if let Some(idx) = last_whitespace {
        idx
    } else {
        byte_index_at_char(text, width)
    }
}

fn char_count(text: &str) -> usize {
    text.chars().count()
}

fn indexed_chars(text: &str) -> impl Iterator<Item = (usize, char, usize)> + '_ {
    text.char_indices()
        .enumerate()
        .map(|(pos, (idx, ch))| (idx, ch, pos + 1))
}

fn byte_index_at_char(text: &str, target_char: usize) -> usize {
    text.char_indices()
        .nth(target_char)
        .map_or(text.len(), |(idx, _)| idx)
}

fn non_empty(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

pub(crate) struct Palette {
    enabled: bool,
}

impl Palette {
    pub(crate) fn auto() -> Self {
        let enabled = std::env::var_os("NO_COLOR").is_none() && io::stdout().is_terminal();
        Self { enabled }
    }

    pub(crate) fn paint(&self, code: &str, text: &str) -> String {
        if self.enabled {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    fn heading(&self, text: &str) -> String {
        self.paint("1;36", text)
    }

    fn label(&self, text: &str) -> String {
        self.paint("36", text)
    }

    fn dim(&self, text: &str) -> String {
        self.paint("2", text)
    }

    pub(crate) fn id(&self, text: &str) -> String {
        self.paint("1;94", text)
    }

    pub(crate) fn state(&self, state: &str) -> String {
        let upper = state.to_ascii_uppercase();
        self.paint(state_color_code(state), &format!("[{upper}]"))
    }

    fn type_label(&self, knot_type: &str) -> String {
        self.paint("35", &format!("({knot_type})"))
    }

    fn tags(&self, text: &str) -> String {
        self.paint("90", text)
    }
}

fn state_color_code(state: &str) -> &'static str {
    match state.trim().to_ascii_lowercase().as_str() {
        // Action states: green
        "planning"
        | "plan_review"
        | "evaluating"
        | "implementation"
        | "implementation_review"
        | "shipment"
        | "shipment_review" => "32",
        // Queue states: yellow
        "ready_for_planning"
        | "ready_for_plan_review"
        | "ready_to_evaluate"
        | "ready_for_implementation"
        | "ready_for_implementation_review"
        | "ready_for_shipment"
        | "ready_for_shipment_review" => "33",
        // Terminal: abandoned = red
        "abandoned" => "31",
        // Terminal: shipped = blue
        "shipped" => "34",
        // Deferred: magenta
        "deferred" => "35",
        // Unknown: default
        _ => "37",
    }
}

struct ShowField {
    label: String,
    value: String,
}

impl ShowField {
    fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{filter_summary, format_show_fields, knot_show_fields, Palette, ShowField};
    use crate::app::KnotView;
    use crate::domain::metadata::MetadataEntry;
    use crate::listing::KnotListFilter;

    #[test]
    fn filter_summary_formats_only_active_filters() {
        let filter = KnotListFilter {
            include_all: false,
            state: Some("implementing".to_string()),
            knot_type: Some("task".to_string()),
            profile_id: Some("default".to_string()),
            tags: vec!["release".to_string(), "".to_string()],
            query: Some("sync".to_string()),
        };

        let summary = filter_summary(&filter).expect("summary should exist");
        assert_eq!(
            summary,
            "state=implementing type=task profile=default tags=release query=sync"
        );
    }

    #[test]
    fn filter_summary_is_none_for_empty_filters() {
        let filter = KnotListFilter::default();
        assert!(filter_summary(&filter).is_none());
    }

    #[test]
    fn filter_summary_includes_all_flag() {
        let filter = KnotListFilter {
            include_all: true,
            state: None,
            knot_type: None,
            profile_id: None,
            tags: Vec::new(),
            query: None,
        };
        let summary = filter_summary(&filter).expect("summary should exist");
        assert_eq!(summary, "all=true");
    }

    #[test]
    fn show_fields_right_align_labels() {
        let fields = vec![
            ShowField::new("id", "knot-123"),
            ShowField::new("profile_id", "default"),
        ];
        let palette = Palette { enabled: false };
        let lines = format_show_fields(&fields, &palette, 80);
        assert_eq!(lines[0], "        id:  knot-123");
        assert_eq!(lines[1], "profile_id:  default");
    }

    #[test]
    fn show_fields_wrap_values_to_width_and_keep_value_column_alignment() {
        let value = format!("{} {}", "a".repeat(40), "b".repeat(50));
        let fields = vec![ShowField::new("body", value)];
        let palette = Palette { enabled: false };
        let lines = format_show_fields(&fields, &palette, 20);
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0], "body:  aaaaaaaaaaaaaaaaaaaa");
        assert_eq!(lines[1], "       aaaaaaaaaaaaaaaaaaaa");
        assert_eq!(lines[2], "       bbbbbbbbbbbbbbbbbbbb");
        assert_eq!(lines[3], "       bbbbbbbbbbbbbbbbbbbb");
        assert_eq!(lines[4], "       bbbbbbbbbb");
    }

    #[test]
    fn knot_show_fields_include_optional_sections() {
        let knot = KnotView {
            id: "knot-123".to_string(),
            alias: Some("alpha".to_string()),
            title: "Fix output formatter".to_string(),
            state: "implementing".to_string(),
            updated_at: "2026-02-25T15:00:00Z".to_string(),
            body: Some("Body text".to_string()),
            description: Some("Description text".to_string()),
            priority: Some(2),
            knot_type: crate::domain::knot_type::KnotType::Work,
            tags: vec!["cli".to_string()],
            notes: vec![MetadataEntry {
                entry_id: "n1".to_string(),
                content: "note".to_string(),
                username: "tester".to_string(),
                datetime: "2026-02-25T15:00:00Z".to_string(),
                agentname: "codex".to_string(),
                model: "gpt-5".to_string(),
                version: "1".to_string(),
            }],
            handoff_capsules: vec![MetadataEntry {
                entry_id: "h1".to_string(),
                content: "handoff".to_string(),
                username: "tester".to_string(),
                datetime: "2026-02-25T15:00:00Z".to_string(),
                agentname: "codex".to_string(),
                model: "gpt-5".to_string(),
                version: "1".to_string(),
            }],
            invariants: vec![],
            step_history: vec![],
            gate: None,
            lease: None,
            lease_id: None,
            workflow_id: "compatibility".to_string(),
            profile_id: "default".to_string(),
            profile_etag: Some("etag-1".to_string()),
            deferred_from_state: None,
            created_at: Some("2026-02-25T14:00:00Z".to_string()),
            edges: vec![],
        };

        let fields = knot_show_fields(&knot, false);
        let labels = fields
            .iter()
            .map(|field| field.label.as_str())
            .collect::<Vec<_>>();
        assert!(labels.contains(&"alias"));
        assert!(labels.contains(&"body"));
        assert!(labels.contains(&"description"));
        assert!(labels.contains(&"priority"));
        assert!(labels.contains(&"type"));
        assert!(labels.contains(&"tags"));
        assert!(labels.contains(&"note"));
        assert!(labels.contains(&"handoff_capsule"));
    }

    fn make_entry(id: &str, content: &str) -> MetadataEntry {
        MetadataEntry {
            entry_id: id.to_string(),
            content: content.to_string(),
            username: "u".to_string(),
            datetime: "2026-02-25T10:00:00Z".to_string(),
            agentname: "a".to_string(),
            model: "m".to_string(),
            version: "v".to_string(),
        }
    }

    fn minimal_knot() -> KnotView {
        KnotView {
            id: "K-1".to_string(),
            alias: None,
            title: "T".to_string(),
            state: "implementing".to_string(),
            updated_at: "2026-02-25T10:00:00Z".to_string(),
            body: None,
            description: None,
            priority: None,
            knot_type: crate::domain::knot_type::KnotType::default(),
            tags: vec![],
            notes: vec![],
            handoff_capsules: vec![],
            invariants: vec![],
            step_history: vec![],
            gate: None,
            lease: None,
            lease_id: None,
            workflow_id: "compatibility".to_string(),
            profile_id: "default".to_string(),
            profile_etag: None,
            deferred_from_state: None,
            created_at: None,
            edges: vec![],
        }
    }

    #[test]
    fn hidden_metadata_hint_empty_when_single_entries() {
        let mut knot = minimal_knot();
        knot.notes = vec![make_entry("n1", "only note")];
        knot.handoff_capsules = vec![make_entry("h1", "only capsule")];
        assert_eq!(super::hidden_metadata_hint(&knot), "");
    }

    #[test]
    fn hidden_metadata_hint_shows_counts_when_multiple() {
        let mut knot = minimal_knot();
        knot.notes = vec![
            make_entry("n1", "old"),
            make_entry("n2", "new"),
            make_entry("n3", "newest"),
        ];
        knot.handoff_capsules = vec![make_entry("h1", "old"), make_entry("h2", "new")];
        let hint = super::hidden_metadata_hint(&knot);
        assert!(hint.contains("2 older notes"));
        assert!(hint.contains("1 older handoff capsule"));
        assert!(hint.contains("-v/--verbose"));
    }

    #[test]
    fn show_fields_verbose_shows_all_entries() {
        let mut knot = minimal_knot();
        knot.notes = vec![
            make_entry("n1", "first note"),
            make_entry("n2", "second note"),
        ];
        let fields = knot_show_fields(&knot, true);
        let note_fields: Vec<_> = fields.iter().filter(|f| f.label == "note").collect();
        assert_eq!(note_fields.len(), 2);
    }

    #[test]
    fn show_fields_non_verbose_shows_only_latest() {
        let mut knot = minimal_knot();
        knot.notes = vec![
            make_entry("n1", "first note"),
            make_entry("n2", "second note"),
        ];
        let fields = knot_show_fields(&knot, false);
        let note_fields: Vec<_> = fields.iter().filter(|f| f.label == "note").collect();
        assert_eq!(note_fields.len(), 1);
        assert!(note_fields[0].value.contains("second note"));
    }

    #[test]
    fn format_entry_inline_uses_agentname_over_username() {
        let entry = make_entry("e1", "content");
        let formatted = super::format_entry_inline(&entry);
        assert!(formatted.contains("[a 2026-02-25]"));
        assert!(formatted.contains("content"));
    }

    #[test]
    fn format_entry_inline_falls_back_to_username() {
        let mut entry = make_entry("e1", "content");
        entry.agentname = "unknown".to_string();
        let formatted = super::format_entry_inline(&entry);
        assert!(formatted.contains("[u 2026-02-25]"));
    }

    #[test]
    fn trim_json_metadata_adds_other_field() {
        let mut knot = minimal_knot();
        knot.notes = vec![make_entry("n1", "old"), make_entry("n2", "new")];
        let mut value = serde_json::to_value(&knot).unwrap();
        super::trim_json_metadata(&mut value, &knot);
        let notes = value["notes"].as_array().unwrap();
        assert_eq!(notes.len(), 1);
        assert!(value["other"].as_str().unwrap().contains("1 older note"));
    }

    #[test]
    fn trim_json_no_other_when_single_entries() {
        let mut knot = minimal_knot();
        knot.notes = vec![make_entry("n1", "only")];
        let mut value = serde_json::to_value(&knot).unwrap();
        super::trim_json_metadata(&mut value, &knot);
        assert!(value.get("other").is_none());
    }

    #[test]
    fn format_knot_show_includes_hint_when_hidden() {
        let mut knot = minimal_knot();
        knot.notes = vec![make_entry("n1", "old"), make_entry("n2", "new")];
        let palette = Palette { enabled: false };
        let lines = super::format_knot_show(&knot, &palette, 80, false);
        let joined = lines.join("\n");
        assert!(joined.contains("1 older note"));
        assert!(joined.contains("-v/--verbose"));
    }

    #[test]
    fn format_knot_show_verbose_omits_hint() {
        let mut knot = minimal_knot();
        knot.notes = vec![make_entry("n1", "old"), make_entry("n2", "new")];
        let palette = Palette { enabled: false };
        let lines = super::format_knot_show(&knot, &palette, 80, true);
        let joined = lines.join("\n");
        assert!(!joined.contains("not shown"));
        let note_count = lines.iter().filter(|l| l.contains("note:")).count();
        assert_eq!(note_count, 2);
    }

    #[test]
    fn knot_show_fields_include_edges_grouped_by_kind() {
        use crate::app::EdgeView;
        let mut knot = minimal_knot();
        knot.edges = vec![
            EdgeView {
                src: "K-1".to_string(),
                kind: "parent_of".to_string(),
                dst: "knots-abc1".to_string(),
            },
            EdgeView {
                src: "K-1".to_string(),
                kind: "parent_of".to_string(),
                dst: "knots-abc2".to_string(),
            },
            EdgeView {
                src: "K-1".to_string(),
                kind: "blocked_by".to_string(),
                dst: "knots-xyz1".to_string(),
            },
            EdgeView {
                src: "knots-other".to_string(),
                kind: "blocks".to_string(),
                dst: "K-1".to_string(),
            },
        ];
        let fields = knot_show_fields(&knot, false);
        let labels: Vec<&str> = fields.iter().map(|f| f.label.as_str()).collect();
        assert!(labels.contains(&"blocked_by"));
        assert!(labels.contains(&"parent_of"));
        assert!(labels.contains(&"blocks (incoming)"));

        let parent_field = fields.iter().find(|f| f.label == "parent_of").unwrap();
        assert!(parent_field.value.contains("abc1"));
        assert!(parent_field.value.contains("abc2"));

        let incoming_field = fields
            .iter()
            .find(|f| f.label == "blocks (incoming)")
            .unwrap();
        assert!(incoming_field.value.contains("other"));
    }

    #[test]
    fn knot_show_fields_no_edges_when_empty() {
        let knot = minimal_knot();
        let fields = knot_show_fields(&knot, false);
        let has_edge_label = fields
            .iter()
            .any(|f| f.label == "parent_of" || f.label == "blocked_by" || f.label == "blocks");
        assert!(!has_edge_label);
    }
}

#[cfg(test)]
#[path = "ui_tests_ext.rs"]
mod tests_ext;
