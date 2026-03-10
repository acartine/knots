use std::path::PathBuf;

use serde_json::Value;
use uuid::Uuid;

use super::{App, AppError, StateActorMetadata, UpdateKnotPatch};

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-app-hierarchy-{}", Uuid::now_v7()));
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
            let value: Value = serde_json::from_slice(&payload).expect("event should parse");
            if value.get("type").and_then(Value::as_str) == Some("knot.state_set") {
                payloads.push(value);
            }
        }
    }
    payloads
}

#[test]
fn parent_cannot_advance_past_direct_child_state() {
    let root = unique_workspace();
    let app = open_app(&root);
    let parent = app
        .create_knot("Parent", None, Some("idea"), Some("default"))
        .expect("parent should be created");
    let child = app
        .create_knot("Child", None, Some("idea"), Some("default"))
        .expect("child should be created");
    app.add_edge(&parent.id, "parent_of", &child.id)
        .expect("edge should be added");

    app.set_state(&child.id, "planning", false, None)
        .expect("child should move to planning");
    app.set_state(&parent.id, "planning", false, None)
        .expect("parent should move to planning");

    let err = app
        .set_state(&parent.id, "ready_for_plan_review", false, None)
        .expect_err("parent should be blocked by child progress");
    match err {
        AppError::HierarchyProgressBlocked {
            knot_id,
            target_state,
            blockers,
        } => {
            assert_eq!(knot_id, parent.id);
            assert_eq!(target_state, "ready_for_plan_review");
            assert_eq!(blockers.len(), 1);
            assert_eq!(blockers[0].id, child.id);
            assert_eq!(blockers[0].state, "planning");
        }
        other => panic!("unexpected error: {other}"),
    }

    let forced = app.set_state_with_actor_and_options(
        &parent.id,
        "ready_for_plan_review",
        true,
        None,
        StateActorMetadata::default(),
        false,
    );
    assert!(
        matches!(forced, Err(AppError::HierarchyProgressBlocked { .. })),
        "--force must not bypass hierarchy progress checks"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn deferred_child_blocks_using_deferred_from_state_progress() {
    let root = unique_workspace();
    let app = open_app(&root);
    let parent = app
        .create_knot("Parent", None, Some("implementation"), Some("default"))
        .expect("parent should be created");
    let child = app
        .create_knot("Child", None, Some("implementation"), Some("default"))
        .expect("child should be created");
    app.add_edge(&parent.id, "parent_of", &child.id)
        .expect("edge should be added");

    let child = app
        .set_state(&child.id, "deferred", false, None)
        .expect("child should defer");
    assert_eq!(child.deferred_from_state.as_deref(), Some("implementation"));

    let patch = UpdateKnotPatch {
        title: None,
        description: None,
        priority: None,
        status: Some("ready_for_implementation_review".to_string()),
        knot_type: None,
        add_tags: vec![],
        remove_tags: vec![],
        add_invariants: vec![],
        remove_invariants: vec![],
        clear_invariants: false,
        add_note: None,
        add_handoff_capsule: None,
        expected_profile_etag: None,
        force: true,
        state_actor: StateActorMetadata::default(),
    };
    let err = app
        .update_knot_with_options(&parent.id, patch, false)
        .expect_err("deferred child should block parent");
    match err {
        AppError::HierarchyProgressBlocked { blockers, .. } => {
            assert_eq!(blockers.len(), 1);
            assert_eq!(blockers[0].id, child.id);
            assert_eq!(
                blockers[0].deferred_from_state.as_deref(),
                Some("implementation")
            );
        }
        other => panic!("unexpected error: {other}"),
    }

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_transition_requires_approval_when_descendants_exist() {
    let root = unique_workspace();
    let app = open_app(&root);
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
        .expect("parent edge should be added");
    app.add_edge(&child.id, "parent_of", &grandchild.id)
        .expect("child edge should be added");

    let err = app
        .set_state_with_actor_and_options(
            &parent.id,
            "abandoned",
            false,
            None,
            StateActorMetadata::default(),
            false,
        )
        .expect_err("terminal parent transition should require approval");
    match err {
        AppError::TerminalCascadeApprovalRequired {
            knot_id,
            target_state,
            descendants,
        } => {
            assert_eq!(knot_id, parent.id);
            assert_eq!(target_state, "abandoned");
            let ids = descendants
                .iter()
                .map(|knot| knot.id.as_str())
                .collect::<Vec<_>>();
            assert_eq!(ids, vec![grandchild.id.as_str(), child.id.as_str()]);
        }
        other => panic!("unexpected error: {other}"),
    }

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn approved_terminal_cascade_updates_descendants_and_marks_events() {
    let root = unique_workspace();
    let app = open_app(&root);
    let parent = app
        .create_knot("Parent", None, Some("implementation"), Some("default"))
        .expect("parent should be created");
    let child = app
        .create_knot("Child", None, Some("planning"), Some("default"))
        .expect("child should be created");
    let grandchild = app
        .create_knot("Grandchild", None, Some("idea"), Some("default"))
        .expect("grandchild should be created");
    let already_terminal = app
        .create_knot("Already terminal", None, Some("shipped"), Some("default"))
        .expect("terminal child should be created");
    app.add_edge(&parent.id, "parent_of", &child.id)
        .expect("parent edge should be added");
    app.add_edge(&child.id, "parent_of", &grandchild.id)
        .expect("child edge should be added");
    app.add_edge(&parent.id, "parent_of", &already_terminal.id)
        .expect("terminal child edge should be added");

    let parent = app
        .set_state_with_actor_and_options(
            &parent.id,
            "abandoned",
            false,
            None,
            StateActorMetadata {
                actor_kind: Some("agent".to_string()),
                agent_name: Some("codex".to_string()),
                agent_model: Some("gpt-5".to_string()),
                agent_version: Some("1".to_string()),
            },
            true,
        )
        .expect("approved cascade should succeed");
    assert_eq!(parent.state, "abandoned");
    assert_eq!(
        app.show_knot(&child.id)
            .expect("child should load")
            .expect("child should exist")
            .state,
        "abandoned"
    );
    assert_eq!(
        app.show_knot(&grandchild.id)
            .expect("grandchild should load")
            .expect("grandchild should exist")
            .state,
        "abandoned"
    );
    assert_eq!(
        app.show_knot(&already_terminal.id)
            .expect("terminal child should load")
            .expect("terminal child should exist")
            .state,
        "shipped"
    );

    let state_events = read_state_events(&root);
    let cascade_events = state_events
        .iter()
        .filter(|event| {
            event["data"]["cascade_approved"].as_bool() == Some(true)
                && event["data"]["cascade_root_id"].as_str() == Some(parent.id.as_str())
        })
        .count();
    assert_eq!(cascade_events, 3);

    let _ = std::fs::remove_dir_all(root);
}
