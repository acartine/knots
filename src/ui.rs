use std::io::{self, IsTerminal};

use crate::list_layout::DisplayKnot;
use crate::listing::KnotListFilter;

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

fn format_knot_row(row: &DisplayKnot, palette: &Palette) -> String {
    let knot = &row.knot;
    let indent = indentation_prefix(row.depth, palette);
    let display_id = match knot.alias.as_deref() {
        Some(alias) => format!("{alias} ({})", knot.id),
        None => knot.id.clone(),
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
    if let Some(workflow_id) = filter.workflow_id.as_deref().and_then(non_empty) {
        parts.push(format!("workflow={workflow_id}"));
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

fn non_empty(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

struct Palette {
    enabled: bool,
}

impl Palette {
    fn auto() -> Self {
        let enabled = std::env::var_os("NO_COLOR").is_none() && io::stdout().is_terminal();
        Self { enabled }
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.enabled {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    fn heading(&self, text: &str) -> String {
        self.paint("1;36", text)
    }

    fn dim(&self, text: &str) -> String {
        self.paint("2", text)
    }

    fn id(&self, text: &str) -> String {
        self.paint("1;94", text)
    }

    fn state(&self, state: &str) -> String {
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
        "idea" => "34",
        "work_item" => "36",
        "implementing" => "33",
        "reviewing" => "35",
        "done" | "closed" => "32",
        "blocked" => "31",
        _ => "37",
    }
}

#[cfg(test)]
mod tests {
    use super::filter_summary;
    use crate::listing::KnotListFilter;

    #[test]
    fn filter_summary_formats_only_active_filters() {
        let filter = KnotListFilter {
            include_all: false,
            state: Some("implementing".to_string()),
            knot_type: Some("task".to_string()),
            workflow_id: Some("default".to_string()),
            tags: vec!["release".to_string(), "".to_string()],
            query: Some("sync".to_string()),
        };

        let summary = filter_summary(&filter).expect("summary should exist");
        assert_eq!(
            summary,
            "state=implementing type=task workflow=default tags=release query=sync"
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
            workflow_id: None,
            tags: Vec::new(),
            query: None,
        };
        let summary = filter_summary(&filter).expect("summary should exist");
        assert_eq!(summary, "all=true");
    }
}
