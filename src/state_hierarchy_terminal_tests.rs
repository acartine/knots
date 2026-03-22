use super::*;
use crate::app::App;
use crate::db::KnotCacheRecord;
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
        acceptance: None,
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
        workflow_id: "compatibility".to_string(),
        profile_id: "default".to_string(),
        profile_etag: None,
        deferred_from_state: deferred_from_state.map(ToString::to_string),
        created_at: None,
    }
}

#[test]
fn terminal_parent_resolutions_require_all_direct_children_and_pick_precedence() {
    let root = unique_workspace();
    let app = open_app(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let shipped_parent = app
        .create_knot(
            "Shipped parent",
            None,
            Some("implementation"),
            Some("default"),
        )
        .expect("parent should be created");
    let shipped_child = app
        .create_knot("Shipped child", None, Some("shipped"), Some("default"))
        .expect("child should be created");
    let deferred_child = app
        .create_knot("Deferred child", None, Some("deferred"), Some("default"))
        .expect("child should be created");
    app.add_edge(&shipped_parent.id, "parent_of", &shipped_child.id)
        .expect("edge should be added");
    app.add_edge(&shipped_parent.id, "parent_of", &deferred_child.id)
        .expect("edge should be added");

    let deferred_parent = app
        .create_knot(
            "Deferred parent",
            None,
            Some("implementation"),
            Some("default"),
        )
        .expect("parent should be created");
    let abandoned_child = app
        .create_knot("Abandoned child", None, Some("abandoned"), Some("default"))
        .expect("child should be created");
    let deferred_only_child = app
        .create_knot(
            "Deferred only child",
            None,
            Some("deferred"),
            Some("default"),
        )
        .expect("child should be created");
    app.add_edge(&deferred_parent.id, "parent_of", &abandoned_child.id)
        .expect("edge should be added");
    app.add_edge(&deferred_parent.id, "parent_of", &deferred_only_child.id)
        .expect("edge should be added");

    let abandoned_parent = app
        .create_knot(
            "Abandoned parent",
            None,
            Some("implementation"),
            Some("default"),
        )
        .expect("parent should be created");
    let abandoned_only_child = app
        .create_knot(
            "Abandoned only child",
            None,
            Some("abandoned"),
            Some("default"),
        )
        .expect("child should be created");
    app.add_edge(&abandoned_parent.id, "parent_of", &abandoned_only_child.id)
        .expect("edge should be added");

    let blocked_parent = app
        .create_knot(
            "Blocked parent",
            None,
            Some("implementation"),
            Some("default"),
        )
        .expect("parent should be created");
    let active_child = app
        .create_knot(
            "Active child",
            None,
            Some("implementation"),
            Some("default"),
        )
        .expect("child should be created");
    app.add_edge(&blocked_parent.id, "parent_of", &active_child.id)
        .expect("edge should be added");

    let conn =
        crate::db::open_connection(db.to_str().expect("db path should be utf8")).expect("db");
    let resolutions = find_terminal_parent_resolutions(&conn).expect("resolutions should load");
    let summary = resolutions
        .into_iter()
        .map(|resolution| (resolution.parent.id, resolution.target_state))
        .collect::<Vec<_>>();

    assert!(summary.contains(&(shipped_parent.id, "shipped".to_string())));
    assert!(summary.contains(&(deferred_parent.id, "deferred".to_string())));
    assert!(summary.contains(&(abandoned_parent.id, "abandoned".to_string())));
    assert!(!summary.iter().any(|(id, _)| id == &blocked_parent.id));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_parent_resolutions_skip_terminal_parents_and_missing_children() {
    let root = unique_workspace();
    let app = open_app(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let shipped_parent = app
        .create_knot("Shipped parent", None, Some("shipped"), Some("default"))
        .expect("parent should be created");
    let shipped_child = app
        .create_knot("Shipped child", None, Some("shipped"), Some("default"))
        .expect("child should be created");
    app.add_edge(&shipped_parent.id, "parent_of", &shipped_child.id)
        .expect("edge should be added");

    let missing_parent = app
        .create_knot(
            "Missing child parent",
            None,
            Some("implementation"),
            Some("default"),
        )
        .expect("parent should be created");
    let conn =
        crate::db::open_connection(db.to_str().expect("db path should be utf8")).expect("db");
    crate::db::insert_edge(&conn, &missing_parent.id, "parent_of", "missing-child")
        .expect("edge should be inserted");

    let resolutions = find_terminal_parent_resolutions(&conn).expect("resolutions should load");
    assert!(resolutions.is_empty());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_resolution_target_handles_deferred_abandoned_and_invalid_states() {
    let deferred = vec![sample_record("child-a", "deferred", None)];
    assert_eq!(
        terminal_resolution_target(&deferred).expect("deferred target should resolve"),
        "deferred"
    );

    let abandoned = vec![sample_record("child-b", "abandoned", None)];
    assert_eq!(
        terminal_resolution_target(&abandoned).expect("abandoned target should resolve"),
        "abandoned"
    );

    let invalid = vec![sample_record("child-c", "implementation", None)];
    let err = terminal_resolution_target(&invalid).expect_err("invalid state should fail");
    assert!(err
        .to_string()
        .contains("non-terminal child state 'implementation'"));
}

#[test]
fn ancestor_terminal_resolutions_walk_parents_once_and_sort_results() {
    let root = unique_workspace();
    let app = open_app(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let grandparent = app
        .create_knot("Grandparent", None, Some("implementation"), Some("default"))
        .expect("grandparent should be created");
    let parent = app
        .create_knot("Parent", None, Some("implementation"), Some("default"))
        .expect("parent should be created");
    let sibling_parent = app
        .create_knot(
            "Sibling parent",
            None,
            Some("implementation"),
            Some("default"),
        )
        .expect("sibling parent should be created");
    let child = app
        .create_knot("Child", None, Some("shipped"), Some("default"))
        .expect("child should be created");
    let sibling_child = app
        .create_knot("Sibling child", None, Some("deferred"), Some("default"))
        .expect("sibling child should be created");

    app.add_edge(&grandparent.id, "parent_of", &parent.id)
        .expect("edge should be added");
    app.add_edge(&parent.id, "parent_of", &child.id)
        .expect("edge should be added");
    app.add_edge(&sibling_parent.id, "parent_of", &child.id)
        .expect("edge should be added");
    app.add_edge(&sibling_parent.id, "parent_of", &sibling_child.id)
        .expect("edge should be added");

    let conn =
        crate::db::open_connection(db.to_str().expect("db path should be utf8")).expect("db");
    let resolutions = find_ancestor_terminal_resolutions(&conn, &child.id)
        .expect("ancestor resolutions should load");
    let mut summary = resolutions
        .into_iter()
        .map(|resolution| (resolution.parent.id, resolution.target_state))
        .collect::<Vec<_>>();
    summary.sort();

    let mut expected = vec![
        (parent.id, "shipped".to_string()),
        (sibling_parent.id, "shipped".to_string()),
    ];
    expected.sort();
    assert_eq!(summary, expected);

    let _ = std::fs::remove_dir_all(root);
}
