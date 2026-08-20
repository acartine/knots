use std::path::Path;

use super::{App, StateActorMetadata, UpdateKnotPatch};

fn unique_workspace() -> knots_test_support::TestWorkspace {
    knots_test_support::workspace("knots-app-auto-resolve")
}

fn open_app(root: &Path) -> App {
    let db = root.join(".knots/cache/state.sqlite");
    App::open(
        db.to_str().expect("db path should be utf8"),
        root.to_path_buf(),
    )
    .expect("app should open")
}

#[test]
fn set_state_terminal_transition_resolves_parent_chain() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let app = open_app(&root);
    let grandparent = app
        .create_knot("Grandparent", None, Some("implementation"), Some("default"))
        .expect("grandparent created");
    let parent = app
        .create_knot("Parent", None, Some("implementation"), Some("default"))
        .expect("parent created");
    let child = app
        .create_knot("Child", None, Some("shipment_review"), Some("default"))
        .expect("child created");
    app.add_edge(&grandparent.id, "parent_of", &parent.id)
        .expect("edge added");
    app.add_edge(&parent.id, "parent_of", &child.id)
        .expect("edge added");

    app.set_state_with_actor_and_options(
        &child.id,
        "shipped",
        false,
        None,
        StateActorMetadata::default(),
        false,
        false,
    )
    .expect("child should ship");

    assert_eq!(app.show_knot(&parent.id).unwrap().unwrap().state, "shipped");
    assert_eq!(
        app.show_knot(&grandparent.id).unwrap().unwrap().state,
        "shipped"
    );
}

#[test]
fn update_terminal_transition_resolves_parent_when_last_child_finishes() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let app = open_app(&root);
    let parent = app
        .create_knot("Parent", None, Some("implementation"), Some("default"))
        .expect("parent created");
    let shipped_child = app
        .create_knot("Shipped child", None, Some("shipped"), Some("default"))
        .expect("shipped child created");
    let pending_child = app
        .create_knot(
            "Pending child",
            None,
            Some("shipment_review"),
            Some("default"),
        )
        .expect("pending child created");
    app.add_edge(&parent.id, "parent_of", &shipped_child.id)
        .expect("edge added");
    app.add_edge(&parent.id, "parent_of", &pending_child.id)
        .expect("edge added");

    let patch = UpdateKnotPatch {
        status: Some("shipped".to_string()),
        state_actor: StateActorMetadata::default(),
        ..UpdateKnotPatch::default()
    };
    app.update_knot_with_options(&pending_child.id, patch, false)
        .expect("child update should ship");

    assert_eq!(app.show_knot(&parent.id).unwrap().unwrap().state, "shipped");
}

#[test]
fn cascade_terminal_transition_resolves_other_parent_of_descendant() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let app = open_app(&root);
    let root_parent = app
        .create_knot("Root parent", None, Some("implementation"), Some("default"))
        .expect("root parent created");
    let other_parent = app
        .create_knot(
            "Other parent",
            None,
            Some("implementation"),
            Some("default"),
        )
        .expect("other parent created");
    let child = app
        .create_knot("Child", None, Some("implementation"), Some("default"))
        .expect("child created");
    app.add_edge(&root_parent.id, "parent_of", &child.id)
        .expect("edge added");
    app.add_edge(&other_parent.id, "parent_of", &child.id)
        .expect("edge added");

    app.set_state_with_actor_and_options(
        &root_parent.id,
        "abandoned",
        false,
        None,
        StateActorMetadata::default(),
        true,
        false,
    )
    .expect("cascade should succeed");

    assert_eq!(
        app.show_knot(&child.id).unwrap().unwrap().state,
        "abandoned"
    );
    assert_eq!(
        app.show_knot(&other_parent.id).unwrap().unwrap().state,
        "abandoned"
    );
}
