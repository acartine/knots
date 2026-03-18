use std::path::PathBuf;

use uuid::Uuid;

use super::{App, StateActorMetadata, UpdateKnotPatch};

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-app-autoresolve-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("temp workspace should be creatable");
    root
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
fn terminal_child_auto_resolves_parent() {
    let root = unique_workspace();
    let app = open_app(&root);
    let parent = app
        .create_knot("Parent", None, Some("implementation"), Some("default"))
        .expect("parent created");
    let child = app
        .create_knot("Child", None, Some("implementation"), Some("default"))
        .expect("child created");
    app.add_edge(&parent.id, "parent_of", &child.id)
        .expect("edge added");

    app.set_state_with_actor_and_options(
        &child.id,
        "abandoned",
        false,
        None,
        StateActorMetadata::default(),
        false,
    )
    .expect("child should abandon");

    assert_eq!(
        app.show_knot(&parent.id).unwrap().unwrap().state,
        "abandoned",
        "parent should auto-resolve to abandoned"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_child_auto_resolves_parent_chain() {
    let root = unique_workspace();
    let app = open_app(&root);
    let grandparent = app
        .create_knot("Grandparent", None, Some("implementation"), Some("default"))
        .expect("grandparent created");
    let parent = app
        .create_knot("Parent", None, Some("implementation"), Some("default"))
        .expect("parent created");
    let child = app
        .create_knot("Child", None, Some("implementation"), Some("default"))
        .expect("child created");
    app.add_edge(&grandparent.id, "parent_of", &parent.id)
        .expect("edge added");
    app.add_edge(&parent.id, "parent_of", &child.id)
        .expect("edge added");

    app.set_state_with_actor_and_options(
        &child.id,
        "abandoned",
        false,
        None,
        StateActorMetadata::default(),
        false,
    )
    .expect("child should abandon");

    assert_eq!(
        app.show_knot(&parent.id).unwrap().unwrap().state,
        "abandoned",
        "parent should auto-resolve to abandoned"
    );
    assert_eq!(
        app.show_knot(&grandparent.id).unwrap().unwrap().state,
        "abandoned",
        "grandparent should auto-resolve to abandoned"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn non_terminal_siblings_prevent_parent_auto_resolve() {
    let root = unique_workspace();
    let app = open_app(&root);
    let parent = app
        .create_knot("Parent", None, Some("implementation"), Some("default"))
        .expect("parent created");
    let child_a = app
        .create_knot("Child A", None, Some("implementation"), Some("default"))
        .expect("child a created");
    let child_b = app
        .create_knot("Child B", None, Some("implementation"), Some("default"))
        .expect("child b created");
    app.add_edge(&parent.id, "parent_of", &child_a.id)
        .expect("edge added");
    app.add_edge(&parent.id, "parent_of", &child_b.id)
        .expect("edge added");

    app.set_state_with_actor_and_options(
        &child_a.id,
        "abandoned",
        false,
        None,
        StateActorMetadata::default(),
        false,
    )
    .expect("child a should abandon");

    assert_eq!(
        app.show_knot(&parent.id).unwrap().unwrap().state,
        "implementation",
        "parent should NOT auto-resolve while child_b is active"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn deferred_child_does_not_trigger_parent_auto_resolve() {
    let root = unique_workspace();
    let app = open_app(&root);
    let parent = app
        .create_knot("Parent", None, Some("implementation"), Some("default"))
        .expect("parent created");
    let child = app
        .create_knot("Child", None, Some("implementation"), Some("default"))
        .expect("child created");
    app.add_edge(&parent.id, "parent_of", &child.id)
        .expect("edge added");

    app.set_state(&child.id, "deferred", false, None)
        .expect("child should defer");

    assert_eq!(
        app.show_knot(&parent.id).unwrap().unwrap().state,
        "implementation",
        "parent should NOT auto-resolve when child is only deferred"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn update_knot_terminal_auto_resolves_parent() {
    let root = unique_workspace();
    let app = open_app(&root);
    let parent = app
        .create_knot("Parent", None, Some("implementation"), Some("default"))
        .expect("parent created");
    let child = app
        .create_knot("Child", None, Some("implementation"), Some("default"))
        .expect("child created");
    app.add_edge(&parent.id, "parent_of", &child.id)
        .expect("edge added");

    let patch = UpdateKnotPatch {
        title: None,
        description: None,
        priority: None,
        status: Some("abandoned".to_string()),
        knot_type: None,
        add_tags: vec![],
        remove_tags: vec![],
        add_invariants: vec![],
        remove_invariants: vec![],
        clear_invariants: false,
        gate_owner_kind: None,
        gate_failure_modes: None,
        clear_gate_failure_modes: false,
        add_note: None,
        add_handoff_capsule: None,
        expected_profile_etag: None,
        force: false,
        state_actor: StateActorMetadata::default(),
    };
    app.update_knot_with_options(&child.id, patch, false)
        .expect("child should abandon via update");

    assert_eq!(
        app.show_knot(&parent.id).unwrap().unwrap().state,
        "abandoned",
        "parent should auto-resolve after child update to terminal"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cascade_auto_resolves_grandparent() {
    let root = unique_workspace();
    let app = open_app(&root);
    let grandparent = app
        .create_knot("Grandparent", None, Some("implementation"), Some("default"))
        .expect("grandparent created");
    let parent = app
        .create_knot("Parent", None, Some("implementation"), Some("default"))
        .expect("parent created");
    let child = app
        .create_knot("Child", None, Some("planning"), Some("default"))
        .expect("child created");
    app.add_edge(&grandparent.id, "parent_of", &parent.id)
        .expect("edge added");
    app.add_edge(&parent.id, "parent_of", &child.id)
        .expect("edge added");

    app.set_state_with_actor_and_options(
        &parent.id,
        "abandoned",
        false,
        None,
        StateActorMetadata::default(),
        true,
    )
    .expect("cascade should succeed");

    assert_eq!(
        app.show_knot(&child.id).unwrap().unwrap().state,
        "abandoned",
        "child should be cascaded to abandoned"
    );
    assert_eq!(
        app.show_knot(&grandparent.id).unwrap().unwrap().state,
        "abandoned",
        "grandparent should auto-resolve after parent cascade"
    );

    let _ = std::fs::remove_dir_all(root);
}
