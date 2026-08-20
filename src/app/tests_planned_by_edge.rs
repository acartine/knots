use super::App;
use crate::db;

fn unique_workspace() -> knots_test_support::TestWorkspace {
    knots_test_support::workspace("knots-app-planned-by")
}

fn open_app(root: &std::path::Path) -> App {
    let db = root.join(".knots/cache/state.sqlite");
    App::open(
        db.to_str().expect("db path should be utf8"),
        root.to_path_buf(),
    )
    .expect("app should open")
}

#[test]
fn planned_by_edge_round_trips_without_hierarchy_or_dependency_side_effects() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let app = open_app(&root);

    let plan = app
        .create_knot("Plan", None, Some("idea"), Some("default"))
        .expect("plan should be created");
    let work = app
        .create_knot("Work", None, Some("idea"), Some("default"))
        .expect("work should be created");

    let edge = app
        .add_edge(&work.id, "planned_by", &plan.id)
        .expect("planned_by edge should be added");
    assert_eq!(edge.kind, "planned_by");
    assert_eq!(edge.src, work.id);
    assert_eq!(edge.dst, plan.id);

    let plan_view = app
        .show_knot(&plan.id)
        .expect("show should succeed")
        .expect("plan should exist");
    assert!(
        plan_view.child_summaries.is_empty(),
        "planned_by must not populate parent_of child summaries: {:?}",
        plan_view.child_summaries
    );

    let layout_kinds: std::collections::HashSet<String> = app
        .list_layout_edges()
        .expect("layout edges should list")
        .into_iter()
        .map(|e| e.kind)
        .collect();
    assert!(
        !layout_kinds.contains("planned_by"),
        "planned_by must be excluded from layout edges (parent_of/blocked_by/blocks only): {:?}",
        layout_kinds
    );

    let work_outgoing: Vec<_> = app
        .list_edges(&work.id, "out")
        .expect("outgoing edges should list")
        .into_iter()
        .filter(|e| e.kind == "planned_by")
        .collect();
    assert_eq!(
        work_outgoing.len(),
        1,
        "edge must be visible via list_edges"
    );

    let work_record = db::get_knot_hot(app.conn_for_test(), &work.id)
        .expect("hot lookup should succeed")
        .expect("work knot should be hot");
    assert!(
        work_record.blocked_from_state.is_none(),
        "planned_by must not set blocked_from_state"
    );

    let removed = app
        .remove_edge(&work.id, "planned_by", &plan.id)
        .expect("planned_by edge should remove");
    assert_eq!(removed.kind, "planned_by");

    let after_remove = app
        .list_edges(&work.id, "out")
        .expect("outgoing edges should list")
        .into_iter()
        .filter(|e| e.kind == "planned_by")
        .count();
    assert_eq!(after_remove, 0, "edge should be gone after remove");

    let work_after = db::get_knot_hot(app.conn_for_test(), &work.id)
        .expect("hot lookup should succeed")
        .expect("work knot should still be hot");
    assert_eq!(
        work_after.state, work.state,
        "removing planned_by must not trigger state changes",
    );
}
