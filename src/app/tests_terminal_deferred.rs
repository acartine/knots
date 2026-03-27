use std::path::PathBuf;

use uuid::Uuid;

use super::{App, StateActorMetadata, UpdateKnotPatch};

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-app-terminal-{}", Uuid::now_v7()));
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
fn update_can_abandon_parent_with_deferred_child_without_auto_resolution() {
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
        "parent should remain active while the child is deferred"
    );

    let parent = app
        .update_knot_with_options(
            &parent.id,
            UpdateKnotPatch {
                title: None,
                description: None,
                acceptance: None,
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
            },
            true,
        )
        .expect("deferred parent should allow terminal update");
    assert_eq!(parent.state, "abandoned");
    assert_eq!(
        app.show_knot(&child.id).unwrap().unwrap().state,
        "abandoned",
        "deferred child should be cascaded to abandoned"
    );

    let _ = std::fs::remove_dir_all(root);
}
