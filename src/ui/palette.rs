use std::io::{self, IsTerminal};
pub(crate) struct Palette {
    pub(super) enabled: bool,
}
impl Palette {
    pub(crate) fn auto() -> Self {
        Self {
            enabled: std::env::var_os("NO_COLOR").is_none() && io::stdout().is_terminal(),
        }
    }
    pub(crate) fn paint(&self, code: &str, text: &str) -> String {
        if self.enabled {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }
    pub(crate) fn heading(&self, text: &str) -> String {
        self.paint("1;36", text)
    }
    pub(crate) fn label(&self, text: &str) -> String {
        self.paint("36", text)
    }
    pub(crate) fn dim(&self, text: &str) -> String {
        self.paint("2", text)
    }
    pub(crate) fn id(&self, text: &str) -> String {
        self.paint("1;94", text)
    }
    pub(crate) fn state(&self, state: &str) -> String {
        let u = state.to_ascii_uppercase();
        self.paint(state_color_code(state), &format!("[{u}]"))
    }
    pub(crate) fn type_label(&self, knot_type: &str) -> String {
        self.paint("35", &format!("({knot_type})"))
    }
    pub(crate) fn tags(&self, text: &str) -> String {
        self.paint("90", text)
    }
}
pub(crate) fn state_color_code(state: &str) -> &'static str {
    match state.trim().to_ascii_lowercase().as_str() {
        "planning"
        | "plan_review"
        | "evaluating"
        | "implementation"
        | "implementation_review"
        | "shipment"
        | "shipment_review" => "32",
        "ready_for_planning"
        | "ready_for_plan_review"
        | "ready_to_evaluate"
        | "ready_for_implementation"
        | "ready_for_implementation_review"
        | "ready_for_shipment"
        | "ready_for_shipment_review" => "33",
        "abandoned" => "31",
        "shipped" => "34",
        "deferred" => "35",
        _ => "37",
    }
}
pub(super) struct ShowField {
    pub(super) label: String,
    pub(super) value: String,
}
impl ShowField {
    pub(super) fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
        }
    }
}
