use crate::app::{EdgeView, KnotView};
use crate::list_layout::layout_knots;

fn knot(id: &str, state: &str) -> KnotView {
    KnotView {
        id: id.to_string(),
        alias: None,
        title: id.to_string(),
        state: state.to_string(),
        updated_at: "2026-02-24T10:00:00Z".to_string(),
        body: None,
        description: None,
        acceptance: None,
        priority: None,
        knot_type: crate::domain::knot_type::KnotType::default(),
        tags: Vec::new(),
        notes: Vec::new(),
        handoff_capsules: Vec::new(),
        invariants: Vec::new(),
        step_history: Vec::new(),
        gate: None,
        lease: None,
        lease_id: None,
        lease_expiry_ts: 0,
        lease_agent: None,
        workflow_id: "knots_sdlc".to_string(),
        profile_id: "default".to_string(),
        profile_etag: None,
        deferred_from_state: None,
        blocked_from_state: None,
        created_at: None,
        step_metadata: None,
        next_step_metadata: None,
        edges: Vec::new(),
        child_summaries: vec![],
    }
}

#[test]
fn renders_children_before_parent_footer() {
    let knots = vec![knot("K-1", "work_item"), knot("K-2", "work_item")];
    let edges = vec![EdgeView {
        src: "K-1".to_string(),
        kind: "parent_of".to_string(),
        dst: "K-2".to_string(),
    }];

    let rows = layout_knots(knots, &edges);
    assert_eq!(rows[0].knot.id, "K-2");
    assert_eq!(rows[0].depth, 0);
    assert_eq!(rows[1].knot.id, "K-1");
    assert_eq!(rows[1].depth, 1);
}

#[test]
fn sequence_order_is_child_specific_then_parent() {
    let knots = vec![
        knot("knots-q3e.5", "blocked"),
        knot("knots-q3e.5.3", "work_item"),
        knot("knots-q3e.5.2", "work_item"),
        knot("knots-q3e.5.1", "work_item"),
    ];

    let rows = layout_knots(knots, &[]);
    assert_eq!(rows[0].knot.id, "knots-q3e.5.1");
    assert_eq!(rows[1].knot.id, "knots-q3e.5.2");
    assert_eq!(rows[2].knot.id, "knots-q3e.5.3");
    assert_eq!(rows[3].knot.id, "knots-q3e.5");
}

#[test]
fn blocked_items_sort_after_actionable_peers() {
    let knots = vec![
        knot("knots-q3e.5", "work_item"),
        knot("knots-q3e.5.3", "work_item"),
        knot("knots-q3e.5.2", "work_item"),
        knot("knots-q3e.5.1", "work_item"),
    ];
    let edges = vec![EdgeView {
        src: "knots-q3e.5".to_string(),
        kind: "blocked_by".to_string(),
        dst: "knots-q3e.5.3".to_string(),
    }];

    let rows = layout_knots(knots, &edges);
    assert_eq!(rows[3].knot.id, "knots-q3e.5");
}

#[test]
fn nested_epic_footer_depth_increases_by_level() {
    let knots = vec![
        knot("knots-q3e", "work_item"),
        knot("knots-q3e.5", "work_item"),
        knot("knots-q3e.5.1", "work_item"),
    ];
    let edges = vec![
        EdgeView {
            src: "knots-q3e".to_string(),
            kind: "parent_of".to_string(),
            dst: "knots-q3e.5".to_string(),
        },
        EdgeView {
            src: "knots-q3e.5".to_string(),
            kind: "parent_of".to_string(),
            dst: "knots-q3e.5.1".to_string(),
        },
    ];

    let rows = layout_knots(knots, &edges);
    assert_eq!(rows[0].knot.id, "knots-q3e.5.1");
    assert_eq!(rows[0].depth, 0);
    assert_eq!(rows[1].knot.id, "knots-q3e.5");
    assert_eq!(rows[1].depth, 1);
    assert_eq!(rows[2].knot.id, "knots-q3e");
    assert_eq!(rows[2].depth, 2);
}

#[test]
fn handles_cycles_without_infinite_loop() {
    let knots = vec![knot("K-1", "work_item"), knot("K-2", "work_item")];
    let edges = vec![
        EdgeView {
            src: "K-1".to_string(),
            kind: "parent_of".to_string(),
            dst: "K-2".to_string(),
        },
        EdgeView {
            src: "K-2".to_string(),
            kind: "parent_of".to_string(),
            dst: "K-1".to_string(),
        },
    ];

    let rows = layout_knots(knots, &edges);
    assert_eq!(rows.len(), 2);
}
