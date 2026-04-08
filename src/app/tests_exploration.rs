use super::{App, AppError, CreateKnotOptions};
use crate::domain::knot_type::KnotType;
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-exploration-test-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("temp workspace");
    root
}

fn open_app(root: &Path) -> App {
    let db_path = root.join(".knots/cache/state.sqlite");
    App::open(db_path.to_str().expect("utf8"), root.to_path_buf()).expect("app should open")
}

fn create_explore_knot(app: &App, title: &str, body: Option<&str>) -> crate::app::KnotView {
    app.create_knot_with_options(
        title,
        body,
        None,
        None,
        None,
        CreateKnotOptions {
            knot_type: KnotType::Explore,
            ..Default::default()
        },
    )
    .expect("create should succeed")
}

#[test]
fn create_exploration_knot_sets_ready_for_exploration() {
    let root = unique_workspace();
    let app = open_app(&root);
    let knot = create_explore_knot(&app, "Investigate caching", None);
    assert_eq!(knot.state, "ready_for_exploration");
    assert_eq!(knot.knot_type, KnotType::Explore);
    assert_eq!(knot.profile_id, "exploration");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn exploration_transitions_to_exploration_state() {
    let root = unique_workspace();
    let app = open_app(&root);
    let knot = create_explore_knot(&app, "Investigate caching", None);
    assert_eq!(knot.state, "ready_for_exploration");

    let updated = app
        .set_state(&knot.id, "exploration", false, None)
        .expect("transition should succeed");
    assert_eq!(updated.state, "exploration");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn exploration_transitions_to_abandoned() {
    let root = unique_workspace();
    let app = open_app(&root);
    let knot = create_explore_knot(&app, "Investigate caching", None);

    let updated = app
        .set_state(&knot.id, "exploration", false, None)
        .expect("transition should succeed");
    assert_eq!(updated.state, "exploration");

    let abandoned = app
        .set_state(&updated.id, "abandoned", false, None)
        .expect("abandon should succeed");
    assert_eq!(abandoned.state, "abandoned");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn exploration_shipped_rejected_without_edges() {
    let root = unique_workspace();
    let app = open_app(&root);
    let knot = create_explore_knot(&app, "Investigate caching", None);
    let knot = app
        .set_state(&knot.id, "exploration", false, None)
        .expect("transition should succeed");

    let err = app
        .set_state(&knot.id, "shipped", false, None)
        .expect_err("shipped should be rejected");
    match err {
        AppError::InvalidArgument(msg) => {
            assert!(
                msg.contains("related knot"),
                "error message should mention related knots: {msg}"
            );
        }
        other => panic!("expected InvalidArgument, got: {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn exploration_shipped_succeeds_with_edge() {
    let root = unique_workspace();
    let app = open_app(&root);
    let knot = create_explore_knot(&app, "Investigate caching", None);
    let knot = app
        .set_state(&knot.id, "exploration", false, None)
        .expect("transition should succeed");

    let outcome = app
        .create_knot_with_options(
            "Cache design doc",
            None,
            None,
            None,
            None,
            CreateKnotOptions {
                knot_type: KnotType::Explore,
                ..Default::default()
            },
        )
        .expect("create outcome should succeed");
    app.add_edge(&knot.id, "relates_to", &outcome.id)
        .expect("edge add should succeed");

    let shipped = app
        .set_state(&knot.id, "shipped", false, None)
        .expect("shipped should succeed with edge");
    assert_eq!(shipped.state, "shipped");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn exploration_abandoned_from_ready_for_exploration() {
    let root = unique_workspace();
    let app = open_app(&root);
    let knot = create_explore_knot(&app, "Investigate caching", None);
    assert_eq!(knot.state, "ready_for_exploration");

    let abandoned = app
        .set_state(&knot.id, "abandoned", false, None)
        .expect("abandon from ready_for_exploration should succeed");
    assert_eq!(abandoned.state, "abandoned");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn exploration_knot_appears_in_list_and_show() {
    let root = unique_workspace();
    let app = open_app(&root);
    let knot = create_explore_knot(
        &app,
        "Investigate caching",
        Some("Evaluate Redis vs Memcached"),
    );

    let listed = app.list_knots().expect("list should succeed");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, knot.id);
    assert_eq!(listed[0].state, "ready_for_exploration");
    assert_eq!(listed[0].profile_id, "exploration");

    let shown = app
        .show_knot(&knot.id)
        .expect("show should succeed")
        .expect("knot should exist");
    assert_eq!(shown.title, "Investigate caching");
    assert_eq!(shown.state, "ready_for_exploration");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn exploration_invalid_transition_rejected() {
    let root = unique_workspace();
    let app = open_app(&root);
    let knot = create_explore_knot(&app, "Investigate caching", None);

    // Cannot skip directly from ready_for_exploration to shipped
    let err = app
        .set_state(&knot.id, "shipped", false, None)
        .expect_err("invalid transition should be rejected");
    // Should fail as an invalid profile transition
    assert!(
        matches!(err, AppError::Workflow(_) | AppError::InvalidTransition(_)),
        "expected workflow/transition error, got: {err:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn exploration_shipped_succeeds_with_incoming_edge() {
    let root = unique_workspace();
    let app = open_app(&root);
    let knot = create_explore_knot(&app, "Investigate caching", None);
    let knot = app
        .set_state(&knot.id, "exploration", false, None)
        .expect("transition should succeed");

    let other = app
        .create_knot_with_options(
            "Parent task",
            None,
            None,
            None,
            None,
            CreateKnotOptions {
                knot_type: KnotType::Explore,
                ..Default::default()
            },
        )
        .expect("create other should succeed");
    // Add incoming edge (other -> this knot)
    app.add_edge(&other.id, "relates_to", &knot.id)
        .expect("edge add should succeed");

    let shipped = app
        .set_state(&knot.id, "shipped", false, None)
        .expect("shipped should succeed with incoming edge");
    assert_eq!(shipped.state, "shipped");
    let _ = std::fs::remove_dir_all(&root);
}
