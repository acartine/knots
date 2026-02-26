use std::io::{self, IsTerminal};

use crate::app::KnotView;
use crate::list_layout::DisplayKnot;
use crate::listing::KnotListFilter;

const SHOW_VALUE_WIDTH: usize = 80;

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

pub fn print_knot_show(knot: &KnotView) {
    let palette = Palette::auto();
    for line in format_knot_show(knot, &palette, SHOW_VALUE_WIDTH) {
        println!("{line}");
    }
}

fn format_knot_row(row: &DisplayKnot, palette: &Palette) -> String {
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

    if let Some(kind) = knot.knot_type.as_deref() {
        line.push(' ');
        line.push_str(&palette.type_label(kind));
    }

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

fn format_knot_show(knot: &KnotView, palette: &Palette, value_width: usize) -> Vec<String> {
    let fields = knot_show_fields(knot);
    format_show_fields(&fields, palette, value_width)
}

fn knot_show_fields(knot: &KnotView) -> Vec<ShowField> {
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
    if let Some(knot_type) = knot.knot_type.as_deref() {
        fields.push(ShowField::new("type", knot_type));
    }
    fields.push(ShowField::new("profile_id", knot.profile_id.clone()));
    if !knot.tags.is_empty() {
        fields.push(ShowField::new("tags", knot.tags.join(", ")));
    }
    if !knot.notes.is_empty() {
        fields.push(ShowField::new("notes", knot.notes.len().to_string()));
    }
    if !knot.handoff_capsules.is_empty() {
        fields.push(ShowField::new(
            "handoff_capsules",
            knot.handoff_capsules.len().to_string(),
        ));
    }
    fields
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
        | "implementation"
        | "implementation_review"
        | "shipment"
        | "shipment_review" => "32",
        // Queue states: yellow
        "ready_for_planning"
        | "ready_for_plan_review"
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
            knot_type: Some("task".to_string()),
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
            profile_id: "default".to_string(),
            profile_etag: Some("etag-1".to_string()),
            deferred_from_state: None,
            created_at: Some("2026-02-25T14:00:00Z".to_string()),
        };

        let fields = knot_show_fields(&knot);
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
        assert!(labels.contains(&"notes"));
        assert!(labels.contains(&"handoff_capsules"));
    }
}

#[cfg(test)]
#[path = "ui_tests_ext.rs"]
mod tests_ext;
