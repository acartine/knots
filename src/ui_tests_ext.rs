use super::{
    format_doctor_line, format_knot_row, format_show_fields, indentation_prefix, knot_show_fields,
    print_doctor_report, print_knot_list, print_knot_show, state_color_code, wrap_split_index,
    wrap_value, Palette, ShowField,
};
use crate::app::KnotView;
use crate::doctor::{DoctorCheck, DoctorReport, DoctorStatus};
use crate::domain::metadata::MetadataEntry;
use crate::list_layout::DisplayKnot;
use crate::listing::KnotListFilter;

fn sample_knot() -> KnotView {
    KnotView {
        id: "K-1".to_string(),
        alias: Some("A.1".to_string()),
        title: "Sample knot".to_string(),
        state: "implementing".to_string(),
        updated_at: "2026-02-25T10:00:00Z".to_string(),
        body: Some("Long body for wrapping".to_string()),
        description: Some("Description".to_string()),
        priority: Some(2),
        knot_type: crate::domain::knot_type::KnotType::Work,
        tags: vec!["alpha".to_string(), "beta".to_string()],
        notes: vec![MetadataEntry {
            entry_id: "n1".to_string(),
            content: "note".to_string(),
            username: "u".to_string(),
            datetime: "2026-02-25T10:00:00Z".to_string(),
            agentname: "a".to_string(),
            model: "m".to_string(),
            version: "v".to_string(),
        }],
        handoff_capsules: vec![MetadataEntry {
            entry_id: "h1".to_string(),
            content: "handoff".to_string(),
            username: "u".to_string(),
            datetime: "2026-02-25T10:00:00Z".to_string(),
            agentname: "a".to_string(),
            model: "m".to_string(),
            version: "v".to_string(),
        }],
        profile_id: "default".to_string(),
        profile_etag: Some("etag".to_string()),
        deferred_from_state: None,
        created_at: Some("2026-02-24T10:00:00Z".to_string()),
    }
}

#[test]
fn row_and_indent_formatting_cover_alias_tag_and_type_paths() {
    let palette = Palette { enabled: false };
    assert_eq!(indentation_prefix(0, &palette), "");
    assert!(indentation_prefix(2, &palette).contains("↳"));

    let row = DisplayKnot {
        knot: sample_knot(),
        depth: 2,
    };
    let formatted = format_knot_row(&row, &palette);
    assert!(formatted.contains("A.1 (1)"));
    assert!(formatted.contains("(work)"));
    assert!(formatted.contains("#alpha #beta"));

    let mut knot = sample_knot();
    knot.alias = None;
    knot.knot_type = crate::domain::knot_type::KnotType::default();
    knot.tags.clear();
    let plain = format_knot_row(&DisplayKnot { knot, depth: 0 }, &palette);
    assert!(plain.starts_with("1 "));
    assert!(!plain.contains('#'));
}

#[test]
fn wrap_helpers_cover_empty_multiline_and_no_whitespace_paths() {
    assert_eq!(wrap_value("", 10), vec![String::new()]);
    assert_eq!(wrap_value("x\r", 10), vec!["x".to_string()]);
    assert_eq!(
        wrap_value("alpha beta\ngamma", 5),
        vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()]
    );

    // Blank line between content triggers wrap_single_line with empty input
    assert_eq!(
        wrap_value("a\n\nb", 10),
        vec!["a".to_string(), String::new(), "b".to_string()]
    );

    let split = wrap_split_index("abcdefgh", 3);
    assert_eq!(split, 3);
}

#[test]
fn palette_and_state_color_cover_all_branches() {
    let enabled = Palette { enabled: true };
    assert!(enabled.paint("36", "x").contains("\u{1b}[36m"));
    assert!(!Palette { enabled: false }
        .paint("36", "x")
        .contains("\u{1b}["));
    assert!(enabled.heading("h").contains('h'));
    assert!(enabled.label("l").contains('l'));
    assert!(enabled.dim("d").contains('d'));
    assert!(enabled.id("i").contains('i'));
    assert!(enabled.state("planning").contains("PLANNING"));
    assert!(enabled.type_label("task").contains("task"));
    assert!(enabled.tags("#x").contains("#x"));

    // WFV2 color mapping
    assert_eq!(state_color_code("planning"), "32");
    assert_eq!(state_color_code("plan_review"), "32");
    assert_eq!(state_color_code("implementation"), "32");
    assert_eq!(state_color_code("implementation_review"), "32");
    assert_eq!(state_color_code("shipment"), "32");
    assert_eq!(state_color_code("shipment_review"), "32");
    assert_eq!(state_color_code("ready_for_planning"), "33");
    assert_eq!(state_color_code("ready_for_implementation"), "33");
    assert_eq!(state_color_code("ready_for_shipment"), "33");
    assert_eq!(state_color_code("abandoned"), "31");
    assert_eq!(state_color_code("shipped"), "34");
    assert_eq!(state_color_code("deferred"), "35");
    assert_eq!(state_color_code("unknown"), "37");
}

#[test]
fn show_and_print_paths_cover_empty_field_and_public_print_functions() {
    let palette = Palette { enabled: false };
    assert!(format_show_fields(&[], &palette, 20).is_empty());

    let fields = knot_show_fields(&sample_knot());
    let lines = format_show_fields(&fields, &palette, 16);
    assert!(!lines.is_empty());
    let label_only = vec![ShowField::new("id", "K-1")];
    assert_eq!(format_show_fields(&label_only, &palette, 8).len(), 1);

    let filter = KnotListFilter {
        include_all: true,
        ..KnotListFilter::default()
    };
    let row = DisplayKnot {
        knot: sample_knot(),
        depth: 1,
    };

    print_knot_list(&[], &filter);
    print_knot_list(&[row], &filter);
    print_knot_show(&sample_knot());
}

#[test]
fn doctor_pass_renders_green_checkmark() {
    let check = DoctorCheck {
        name: "lock_health".to_string(),
        status: DoctorStatus::Pass,
        detail: "all good".to_string(),
    };
    let palette = Palette { enabled: true };
    let line = format_doctor_line(&check, &palette);
    assert!(line.contains("\u{2713}"), "should contain checkmark");
    assert!(line.contains("\x1b[32m"), "should contain green ANSI code");
    assert!(line.contains("lock_health"));
    assert!(line.contains("all good"));
}

#[test]
fn doctor_warn_renders_yellow_warning() {
    let check = DoctorCheck {
        name: "lock_health".to_string(),
        status: DoctorStatus::Warn,
        detail: "locks busy".to_string(),
    };
    let palette = Palette { enabled: true };
    let line = format_doctor_line(&check, &palette);
    assert!(line.contains("\u{26a0}"), "should contain warning icon");
    assert!(line.contains("\x1b[33m"), "should contain yellow ANSI code");
}

#[test]
fn doctor_fail_renders_red_x() {
    let check = DoctorCheck {
        name: "worktree".to_string(),
        status: DoctorStatus::Fail,
        detail: "not a git repo".to_string(),
    };
    let palette = Palette { enabled: true };
    let line = format_doctor_line(&check, &palette);
    assert!(line.contains("\u{2717}"), "should contain X mark");
    assert!(line.contains("\x1b[31m"), "should contain red ANSI code");
}

#[test]
fn doctor_no_color_omits_ansi_codes() {
    let check = DoctorCheck {
        name: "remote".to_string(),
        status: DoctorStatus::Pass,
        detail: "origin reachable".to_string(),
    };
    let palette = Palette { enabled: false };
    let line = format_doctor_line(&check, &palette);
    assert!(!line.contains("\x1b["), "should not contain ANSI codes");
    assert!(line.contains("\u{2713}"));
    assert!(line.contains("remote"));
    assert!(line.contains("origin reachable"));
}

#[test]
fn print_doctor_report_covers_all_statuses() {
    let report = DoctorReport {
        checks: vec![
            DoctorCheck {
                name: "a".to_string(),
                status: DoctorStatus::Pass,
                detail: "ok".to_string(),
            },
            DoctorCheck {
                name: "b".to_string(),
                status: DoctorStatus::Warn,
                detail: "meh".to_string(),
            },
            DoctorCheck {
                name: "c".to_string(),
                status: DoctorStatus::Fail,
                detail: "bad".to_string(),
            },
        ],
    };
    print_doctor_report(&report);
}
