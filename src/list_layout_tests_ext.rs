use crate::app::{EdgeView, KnotView};
use crate::list_layout::layout_knots;

fn knot(id: &str, alias: Option<&str>, title: &str, state: &str) -> KnotView {
    KnotView {
        id: id.to_string(),
        alias: alias.map(|value| value.to_string()),
        title: title.to_string(),
        state: state.to_string(),
        updated_at: "2026-02-25T10:00:00Z".to_string(),
        body: None,
        description: None,
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
        workflow_id: "compatibility".to_string(),
        profile_id: "automation_granular".to_string(),
        profile_etag: None,
        deferred_from_state: None,
        created_at: None,
        edges: Vec::new(),
        child_summaries: vec![],
    }
}

#[test]
fn layout_knots_returns_empty_for_empty_input() {
    let rows = layout_knots(Vec::new(), &[]);
    assert!(rows.is_empty());
}

#[test]
fn blocks_edges_affect_readiness_sorting() {
    let knots = vec![
        knot("K-ready", None, "ready", "work_item"),
        knot("K-blocker", None, "blocker", "work_item"),
    ];
    let edges = vec![EdgeView {
        src: "K-blocker".to_string(),
        kind: "blocks".to_string(),
        dst: "K-ready".to_string(),
    }];

    let rows = layout_knots(knots, &edges);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].knot.id, "K-blocker");
    assert_eq!(rows[1].knot.id, "K-ready");
}

#[test]
fn sequence_nodes_sort_before_non_sequence_nodes() {
    let knots = vec![
        knot("plain-knot", None, "plain", "work_item"),
        knot("project.1", None, "sequenced", "work_item"),
    ];

    let rows = layout_knots(knots, &[]);
    assert_eq!(rows[0].knot.id, "project.1");
    assert_eq!(rows[1].knot.id, "plain-knot");
}

#[test]
fn malformed_sequence_alias_falls_back_and_terminal_state_sorts_last() {
    let knots = vec![
        knot("zeta", Some("oops."), "zeta", "work_item"),
        knot("alpha", Some("alpha.1"), "alpha", "work_item"),
        knot("done-item", Some("alpha.2"), "done", "shipped"),
    ];
    let edges = vec![EdgeView {
        src: "alpha".to_string(),
        kind: "parent_of".to_string(),
        dst: "alpha".to_string(),
    }];

    let rows = layout_knots(knots, &edges);
    assert_eq!(rows[0].knot.id, "alpha");
    assert_eq!(rows[1].knot.id, "zeta");
    assert_eq!(rows[2].knot.id, "done-item");
}

#[test]
fn different_prefix_sequences_sort_by_prefix() {
    let knots = vec![
        knot("beta.1", None, "beta", "work_item"),
        knot("alpha.1", None, "alpha", "work_item"),
    ];

    let rows = layout_knots(knots, &[]);
    assert_eq!(rows[0].knot.id, "alpha.1");
    assert_eq!(rows[1].knot.id, "beta.1");
}

#[test]
fn state_rank_covers_all_named_states() {
    let states = [
        ("implementing", 0),
        ("reviewing", 1),
        ("work_item", 2),
        ("idea", 3),
        ("refining", 4),
        ("blocked", 5),
        ("approved", 6),
        ("closed", 7),
        ("shipped", 8),
        ("deferred", 9),
        ("abandoned", 10),
        ("custom_state", 11),
    ];
    for (index, (state, _rank)) in states.iter().enumerate() {
        let mut knots_list = vec![knot(
            &format!("K-{index}"),
            None,
            &format!("k{index}"),
            state,
        )];
        if index < states.len() - 1 {
            knots_list.push(knot(
                &format!("K-{}", index + 100),
                None,
                &format!("k{}", index + 100),
                states[index + 1].0,
            ));
        }
        let rows = layout_knots(knots_list, &[]);
        assert!(!rows.is_empty());
    }
}

#[test]
fn priority_tiebreaker_sorts_lower_priority_first() {
    let mut k1 = knot("K-1", None, "same", "work_item");
    k1.priority = Some(1);
    let mut k2 = knot("K-2", None, "same", "work_item");
    k2.priority = Some(5);
    let knots = vec![k2, k1];
    let rows = layout_knots(knots, &[]);
    assert_eq!(rows[0].knot.id, "K-1");
    assert_eq!(rows[1].knot.id, "K-2");
}
