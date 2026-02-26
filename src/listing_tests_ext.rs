use super::{apply_filters, normalize_knot_type_filter, KnotListFilter};
use crate::app::KnotView;

fn knot(
    id: &str,
    title: &str,
    state: &str,
    knot_type: Option<&str>,
    tags: &[&str],
    description: Option<&str>,
) -> KnotView {
    KnotView {
        id: id.to_string(),
        alias: None,
        title: title.to_string(),
        state: state.to_string(),
        updated_at: "2026-02-23T10:00:00Z".to_string(),
        body: None,
        description: description.map(|value| value.to_string()),
        priority: None,
        knot_type: crate::domain::knot_type::parse_knot_type(knot_type),
        tags: tags.iter().map(|value| (*value).to_string()).collect(),
        notes: Vec::new(),
        handoff_capsules: Vec::new(),
        profile_id: "default".to_string(),
        profile_etag: None,
        deferred_from_state: None,
        created_at: None,
    }
}

#[test]
fn filters_by_type_normalizes_legacy_aliases() {
    let knots = vec![
        knot("K-1", "Alpha", "work_item", Some("work"), &[], None),
        knot("K-2", "Beta", "work_item", Some("task"), &[], None),
    ];
    let filter = KnotListFilter {
        include_all: false,
        state: None,
        knot_type: Some("task".to_string()),
        profile_id: None,
        tags: Vec::new(),
        query: None,
    };

    let filtered = apply_filters(knots, &filter);
    assert_eq!(filtered.len(), 2);
}

#[test]
fn invalid_type_filter_is_ignored() {
    let knots = vec![knot("K-1", "Alpha", "work_item", Some("work"), &[], None)];
    let filter = KnotListFilter {
        include_all: false,
        state: None,
        knot_type: Some("epic".to_string()),
        profile_id: None,
        tags: Vec::new(),
        query: None,
    };

    let filtered = apply_filters(knots, &filter);
    assert_eq!(filtered.len(), 1);
}

#[test]
fn include_all_with_user_filter_includes_terminal_knots() {
    let knots = vec![
        knot("K-1", "Active", "implementing", Some("work"), &[], None),
        knot("K-2", "Done", "shipped", Some("work"), &["cli"], None),
    ];
    let filter = KnotListFilter {
        include_all: true,
        state: None,
        knot_type: None,
        profile_id: None,
        tags: vec!["cli".to_string()],
        query: None,
    };

    let filtered = apply_filters(knots, &filter);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, "K-2");
}

#[test]
fn normalize_knot_type_filter_covers_edge_cases() {
    assert_eq!(normalize_knot_type_filter(None), None);
    assert_eq!(normalize_knot_type_filter(Some("")), None);
    assert_eq!(normalize_knot_type_filter(Some("  ")), None);
    assert_eq!(
        normalize_knot_type_filter(Some("task")),
        Some("work".to_string())
    );
    assert_eq!(
        normalize_knot_type_filter(Some("work")),
        Some("work".to_string())
    );
    assert_eq!(normalize_knot_type_filter(Some("epic")), None);
}

#[test]
fn empty_state_filter_is_treated_as_no_filter() {
    let knots = vec![
        knot("K-1", "Active", "implementing", Some("work"), &[], None),
        knot("K-2", "Other", "work_item", Some("work"), &[], None),
    ];
    let filter = KnotListFilter {
        include_all: false,
        state: Some("".to_string()),
        knot_type: None,
        profile_id: None,
        tags: Vec::new(),
        query: None,
    };

    let filtered = apply_filters(knots, &filter);
    assert_eq!(filtered.len(), 2);
}

#[test]
fn whitespace_only_query_filter_is_treated_as_no_filter() {
    let knots = vec![knot(
        "K-1",
        "Active",
        "implementing",
        Some("work"),
        &[],
        None,
    )];
    let filter = KnotListFilter {
        include_all: false,
        state: None,
        knot_type: None,
        profile_id: None,
        tags: Vec::new(),
        query: Some("   ".to_string()),
    };

    let filtered = apply_filters(knots, &filter);
    assert_eq!(filtered.len(), 1);
}
