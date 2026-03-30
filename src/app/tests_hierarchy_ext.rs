use super::{App, AppError, StateActorMetadata};

use std::path::PathBuf;

use serde_json::Value;
use uuid::Uuid;

fn unique_workspace() -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("knots-app-hierarchy-ext-{}", Uuid::now_v7()));
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

fn read_state_events(root: &std::path::Path) -> Vec<Value> {
    let mut payloads = Vec::new();
    let mut stack = vec![root.join(".knots/events")];
    while let Some(dir) = stack.pop() {
        if !dir.exists() {
            continue;
        }
        for entry in std::fs::read_dir(dir).expect("events directory should read") {
            let path = entry.expect("dir entry should read").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let payload = std::fs::read(&path).expect("event file should read");
            let value: Value =
                serde_json::from_slice(&payload).expect("event should parse");
            if value.get("type").and_then(Value::as_str) == Some("knot.state_set") {
                payloads.push(value);
            }
        }
    }
    payloads
}

#[test]
fn deferred_descendant_is_cascaded_in_terminal_transition() {
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

    let parent = app
        .set_state_with_actor_and_options(
            &parent.id,
            "abandoned",
            false,
            None,
            StateActorMetadata::default(),
            true,
            false,
        )
        .expect("cascade should include deferred child");
    assert_eq!(parent.state, "abandoned");
    assert_eq!(
        app.show_knot(&child.id).unwrap().unwrap().state,
        "abandoned",
        "deferred child should be cascaded to abandoned"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn recursive_cascade_reaches_great_grandchildren() {
    let root = unique_workspace();
    let app = open_app(&root);
    let parent = app
        .create_knot("Parent", None, Some("implementation"), Some("default"))
        .expect("parent created");
    let child = app
        .create_knot("Child", None, Some("planning"), Some("default"))
        .expect("child created");
    let grandchild = app
        .create_knot("Grandchild", None, Some("idea"), Some("default"))
        .expect("grandchild created");
    let great_grandchild = app
        .create_knot("Great-grandchild", None, Some("idea"), Some("default"))
        .expect("great-grandchild created");
    app.add_edge(&parent.id, "parent_of", &child.id)
        .expect("edge added");
    app.add_edge(&child.id, "parent_of", &grandchild.id)
        .expect("edge added");
    app.add_edge(&grandchild.id, "parent_of", &great_grandchild.id)
        .expect("edge added");

    let parent = app
        .set_state_with_actor_and_options(
            &parent.id,
            "abandoned",
            false,
            None,
            StateActorMetadata::default(),
            true,
            false,
        )
        .expect("approved cascade should succeed");
    assert_eq!(parent.state, "abandoned");
    for id in [&child.id, &grandchild.id, &great_grandchild.id] {
        assert_eq!(
            app.show_knot(id).unwrap().unwrap().state,
            "abandoned",
            "{id} should be abandoned"
        );
    }

    let state_events = read_state_events(&root);
    let cascade_events = state_events
        .iter()
        .filter(|e| {
            e["data"]["cascade_approved"].as_bool() == Some(true)
                && e["data"]["cascade_root_id"].as_str()
                    == Some(parent.id.as_str())
        })
        .count();
    assert_eq!(
        cascade_events, 4,
        "parent + 3 descendants = 4 cascade events"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn only_behind_children_appear_as_blockers() {
    let root = unique_workspace();
    let app = open_app(&root);
    let parent = app
        .create_knot("Parent", None, Some("idea"), Some("default"))
        .expect("parent created");
    let behind = app
        .create_knot("Behind", None, Some("idea"), Some("default"))
        .expect("behind child created");
    let ahead = app
        .create_knot("Ahead", None, Some("idea"), Some("default"))
        .expect("ahead child created");
    app.add_edge(&parent.id, "parent_of", &behind.id)
        .expect("edge added");
    app.add_edge(&parent.id, "parent_of", &ahead.id)
        .expect("edge added");

    app.set_state(&behind.id, "planning", false, None)
        .expect("behind moves to planning");
    app.set_state(&ahead.id, "planning", false, None)
        .expect("ahead moves to planning");
    app.set_state(&ahead.id, "ready_for_plan_review", false, None)
        .expect("ahead moves to ready_for_plan_review");
    app.set_state(&parent.id, "planning", false, None)
        .expect("parent moves to planning");

    let err = app
        .set_state(&parent.id, "ready_for_plan_review", false, None)
        .expect_err("behind child should block");
    match err {
        AppError::HierarchyProgressBlocked { blockers, .. } => {
            assert_eq!(blockers.len(), 1, "only the behind child should block");
            assert_eq!(blockers[0].id, behind.id);
        }
        other => panic!("unexpected error: {other}"),
    }

    let _ = std::fs::remove_dir_all(root);
}
