use super::*;
use crate::app::{App, CreateKnotOptions};
use crate::db::KnotCacheRecord;
use crate::domain::gate::GateData;
use crate::domain::knot_type::KnotType;
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-state-hierarchy-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("temp workspace should be creatable");
    root
}

fn open_app(root: &Path) -> App {
    let db = root.join(".knots/cache/state.sqlite");
    App::open(
        db.to_str().expect("db path should be utf8"),
        root.to_path_buf(),
    )
    .expect("app should open")
}

fn sample_record(id: &str, state: &str, deferred_from_state: Option<&str>) -> KnotCacheRecord {
    KnotCacheRecord {
        id: id.to_string(),
        title: id.to_string(),
        state: state.to_string(),
        updated_at: "2026-03-10T00:00:00Z".to_string(),
        body: None,
        description: None,
        priority: None,
        knot_type: None,
        tags: Vec::new(),
        notes: Vec::new(),
        handoff_capsules: Vec::new(),
        invariants: Vec::new(),
        step_history: Vec::new(),
        gate_data: crate::domain::gate::GateData::default(),
        lease_data: crate::domain::lease::LeaseData::default(),
        lease_id: None,
        profile_id: "default".to_string(),
        profile_etag: None,
        deferred_from_state: deferred_from_state.map(ToString::to_string),
        created_at: None,
    }
}

#[test]
fn hierarchy_knot_formats_deferred_state_with_provenance() {
    let knot = HierarchyKnot::from_record(&sample_record(
        "knots-child",
        "deferred",
        Some("implementation"),
    ));
    assert_eq!(knot.display_state(), "deferred from implementation");
}

#[test]
fn target_rank_uses_current_progress_when_deferring() {
    let knot = sample_record("knots-parent", "implementation_review", None);
    let rank = effective_target_rank(&knot, "deferred").expect("rank should resolve");
    assert_eq!(rank, 7);
}

#[test]
fn record_rank_uses_deferred_from_state() {
    let knot = sample_record("knots-child", "deferred", Some("plan_review"));
    let rank = effective_record_rank(&knot).expect("rank should resolve");
    assert_eq!(rank, 3);
}

#[test]
fn terminal_state_helper_matches_terminal_states() {
    assert!(is_terminal_state("shipped").expect("shipped should parse"));
    assert!(is_terminal_state("abandoned").expect("abandoned should parse"));
    assert!(!is_terminal_state("implementation").expect("implementation should parse"));
}

#[test]
fn format_hierarchy_knots_lists_each_knot_and_display_state() {
    let rendered = format_hierarchy_knots(&[
        HierarchyKnot::from_record(&sample_record("knots-a", "planning", None)),
        HierarchyKnot::from_record(&sample_record(
            "knots-b",
            "deferred",
            Some("implementation"),
        )),
    ]);
    assert!(rendered.contains("knots-a [planning]"));
    assert!(rendered.contains("knots-b [deferred from implementation]"));
}

#[test]
fn plan_state_transition_blocks_direct_children_that_are_behind() {
    let root = unique_workspace();
    let app = open_app(&root);
    let db = root.join(".knots/cache/state.sqlite");
    let parent = app
        .create_knot("Parent", None, Some("planning"), Some("default"))
        .expect("parent should be created");
    let child = app
        .create_knot("Child", None, Some("planning"), Some("default"))
        .expect("child should be created");
    app.add_edge(&parent.id, "parent_of", &child.id)
        .expect("edge should be added");
    let conn =
        crate::db::open_connection(db.to_str().expect("db path should be utf8")).expect("db");
    let parent = crate::db::get_knot_hot(&conn, &parent.id)
        .expect("db lookup should succeed")
        .expect("parent should exist");

    let err = plan_state_transition(&conn, &parent, "ready_for_plan_review", false, false)
        .expect_err("direct child should block parent");
    assert!(matches!(err, AppError::HierarchyProgressBlocked { .. }));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn gate_parent_transition_blocks_work_child_with_lower_effective_rank() {
    let root = unique_workspace();
    let app = open_app(&root);
    let db = root.join(".knots/cache/state.sqlite");
    let parent = app
        .create_knot_with_options(
            "Gate parent",
            None,
            None,
            None,
            CreateKnotOptions {
                knot_type: KnotType::Gate,
                gate_data: GateData::default(),
                ..CreateKnotOptions::default()
            },
        )
        .expect("gate parent should be created");
    let child = app
        .create_knot("Work child", None, Some("shipment_review"), Some("default"))
        .expect("work child should be created");
    app.add_edge(&parent.id, "parent_of", &child.id)
        .expect("edge should be added");
    let conn =
        crate::db::open_connection(db.to_str().expect("db path should be utf8")).expect("db");
    let parent = crate::db::get_knot_hot(&conn, &parent.id)
        .expect("db lookup should succeed")
        .expect("parent should exist");

    let err = plan_state_transition(&conn, &parent, "evaluating", false, false)
        .expect_err("gate parent should be blocked by work child progress");
    match err {
        AppError::HierarchyProgressBlocked { blockers, .. } => {
            assert_eq!(blockers.len(), 1);
            assert_eq!(blockers[0].id, child.id);
            assert_eq!(blockers[0].state, "shipment_review");
        }
        other => panic!("unexpected error: {other}"),
    }

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn plan_state_transition_returns_sorted_descendants_for_terminal_cascade() {
    let root = unique_workspace();
    let app = open_app(&root);
    let db = root.join(".knots/cache/state.sqlite");
    let parent = app
        .create_knot("Parent", None, Some("implementation"), Some("default"))
        .expect("parent should be created");
    let child = app
        .create_knot("Child", None, Some("planning"), Some("default"))
        .expect("child should be created");
    let grandchild = app
        .create_knot("Grandchild", None, Some("idea"), Some("default"))
        .expect("grandchild should be created");
    app.add_edge(&parent.id, "parent_of", &child.id)
        .expect("edge should be added");
    app.add_edge(&child.id, "parent_of", &grandchild.id)
        .expect("edge should be added");
    let conn =
        crate::db::open_connection(db.to_str().expect("db path should be utf8")).expect("db");
    let parent = crate::db::get_knot_hot(&conn, &parent.id)
        .expect("db lookup should succeed")
        .expect("parent should exist");

    let plan = plan_state_transition(&conn, &parent, "abandoned", true, true)
        .expect("approved terminal cascade should plan");
    match plan {
        TransitionPlan::CascadeTerminal { descendants } => {
            let ids = descendants
                .iter()
                .map(|knot| knot.id.as_str())
                .collect::<Vec<_>>();
            assert_eq!(ids, vec![grandchild.id.as_str(), child.id.as_str()]);
        }
        other => panic!("unexpected plan: {other:?}"),
    }

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn no_op_transition_is_allowed() {
    let root = unique_workspace();
    let app = open_app(&root);
    let knot = app
        .create_knot("Parent", None, Some("planning"), Some("default"))
        .expect("knot should be created");
    let db = root.join(".knots/cache/state.sqlite");
    let conn =
        crate::db::open_connection(db.to_str().expect("db path should be utf8")).expect("db");
    let knot = crate::db::get_knot_hot(&conn, &knot.id)
        .expect("db lookup should succeed")
        .expect("knot should exist");

    let plan = plan_state_transition(&conn, &knot, "planning", false, false).expect("plan works");
    assert!(matches!(plan, TransitionPlan::Allowed));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_plan_without_descendants_is_allowed() {
    let root = unique_workspace();
    let app = open_app(&root);
    let parent = app
        .create_knot("Solo", None, Some("implementation"), Some("default"))
        .expect("parent should be created");
    let db = root.join(".knots/cache/state.sqlite");
    let conn =
        crate::db::open_connection(db.to_str().expect("db path should be utf8")).expect("db");
    let parent = crate::db::get_knot_hot(&conn, &parent.id)
        .expect("db lookup should succeed")
        .expect("parent should exist");

    let plan = plan_state_transition(&conn, &parent, "abandoned", true, false).expect("plan works");
    assert!(matches!(plan, TransitionPlan::Allowed));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_plan_requires_approval_when_descendants_exist() {
    let root = unique_workspace();
    let app = open_app(&root);
    let parent = app
        .create_knot("Parent", None, Some("implementation"), Some("default"))
        .expect("parent should be created");
    let child = app
        .create_knot("Child", None, Some("planning"), Some("default"))
        .expect("child should be created");
    app.add_edge(&parent.id, "parent_of", &child.id)
        .expect("edge should be added");
    let db = root.join(".knots/cache/state.sqlite");
    let conn =
        crate::db::open_connection(db.to_str().expect("db path should be utf8")).expect("db");
    let parent = crate::db::get_knot_hot(&conn, &parent.id)
        .expect("db lookup should succeed")
        .expect("parent should exist");

    let err = plan_state_transition(&conn, &parent, "abandoned", true, false)
        .expect_err("approval should be required");
    match err {
        AppError::TerminalCascadeApprovalRequired { descendants, .. } => {
            assert_eq!(descendants.len(), 1);
            assert_eq!(descendants[0].id, child.id);
        }
        other => panic!("unexpected error: {other}"),
    }

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn collect_descendant_depths_skips_cycles_and_keeps_deepest_path() {
    let child_graph = HashMap::from([
        (
            "root".to_string(),
            vec!["child".to_string(), "middle".to_string()],
        ),
        ("middle".to_string(), vec!["child".to_string()]),
        ("child".to_string(), vec!["root".to_string()]),
    ]);
    let mut path = HashSet::from(["root".to_string()]);
    let mut depths = HashMap::new();

    collect_descendant_depths(&child_graph, "root", 1, &mut path, &mut depths);

    assert_eq!(depths.get("child"), Some(&2));
    assert_eq!(depths.get("middle"), Some(&1));
    assert!(!depths.contains_key("root"));
}

#[test]
fn deferred_target_without_provenance_uses_deferred_rank() {
    let knot = sample_record("knots-child", "deferred", None);
    let rank = effective_target_rank(&knot, "deferred").expect("rank should resolve");
    assert_eq!(rank, 255);
}

#[test]
fn effective_state_rank_covers_remaining_shipment_and_terminal_states() {
    assert_eq!(
        effective_state_rank("ready_for_shipment").expect("state should parse"),
        8
    );
    assert_eq!(
        effective_state_rank("shipment").expect("state should parse"),
        9
    );
    assert_eq!(
        effective_state_rank("ready_for_shipment_review").expect("state should parse"),
        10
    );
    assert_eq!(
        effective_state_rank("shipment_review").expect("state should parse"),
        11
    );
    assert_eq!(
        effective_state_rank("shipped").expect("state should parse"),
        14
    );
    assert_eq!(
        effective_state_rank("abandoned").expect("state should parse"),
        14
    );
    assert_eq!(
        effective_state_rank("deferred").expect("state should parse"),
        255
    );
}

#[test]
fn effective_state_rank_assigns_unique_ranks_to_gate_states() {
    assert_eq!(
        effective_state_rank("ready_to_evaluate").expect("state should parse"),
        12
    );
    assert_eq!(
        effective_state_rank("evaluating").expect("state should parse"),
        13
    );
}
