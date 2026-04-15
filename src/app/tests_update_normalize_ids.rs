use super::{App, AppError, UpdateKnotPatch};
use crate::db::open_connection;
use crate::domain::execution_plan::{
    ExecutionPlanData, ExecutionPlanKnot, ExecutionPlanStep, ExecutionPlanWave,
};
use crate::knot_id::display_id;
use rusqlite::params;
use std::path::PathBuf;
use uuid::Uuid;

fn workspace() -> PathBuf {
    let r = std::env::temp_dir().join(format!("knots-norm-test-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&r).expect("mkdir");
    r
}

fn patch(ep: ExecutionPlanData) -> UpdateKnotPatch {
    UpdateKnotPatch {
        execution_plan_data: Some(ep),
        ..Default::default()
    }
}

#[test]
fn bare_ids_are_normalized_to_fully_qualified() {
    let root = workspace();
    let db = root.join(".knots/cache/state.sqlite");
    let app = App::open(db.to_str().unwrap(), root.clone()).unwrap();
    let parent = app
        .create_knot("Plan", None, Some("idea"), Some("default"))
        .unwrap();
    let child = app
        .create_knot("Task", None, Some("idea"), Some("default"))
        .unwrap();

    let bare = display_id(&child.id).to_string();
    let ep = ExecutionPlanData {
        knot_ids: vec![bare.clone()],
        unassigned_knot_ids: vec![bare.clone()],
        waves: vec![ExecutionPlanWave {
            knots: vec![ExecutionPlanKnot {
                id: bare.clone(),
                title: "task".into(),
            }],
            steps: vec![ExecutionPlanStep {
                step_index: 1,
                knot_ids: vec![bare],
                notes: None,
            }],
            ..Default::default()
        }],
        ..Default::default()
    };

    let updated = app
        .update_knot(&parent.id, patch(ep))
        .expect("update should succeed");

    let plan = updated.execution_plan.expect("plan present");
    assert_eq!(plan.knot_ids, vec![child.id.clone()]);
    assert_eq!(plan.unassigned_knot_ids, vec![child.id.clone()],);
    assert_eq!(plan.waves[0].knots[0].id, child.id);
    assert_eq!(plan.waves[0].steps[0].knot_ids, vec![child.id.clone()],);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn already_qualified_ids_pass_through_unchanged() {
    let root = workspace();
    let db = root.join(".knots/cache/state.sqlite");
    let app = App::open(db.to_str().unwrap(), root.clone()).unwrap();
    let parent = app
        .create_knot("Plan", None, Some("idea"), Some("default"))
        .unwrap();
    let child = app
        .create_knot("Task", None, Some("idea"), Some("default"))
        .unwrap();

    let ep = ExecutionPlanData {
        knot_ids: vec![child.id.clone()],
        ..Default::default()
    };

    let updated = app
        .update_knot(&parent.id, patch(ep))
        .expect("update should succeed");

    let plan = updated.execution_plan.expect("plan present");
    assert_eq!(plan.knot_ids, vec![child.id]);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn unresolvable_ids_are_rejected() {
    let root = workspace();
    let db = root.join(".knots/cache/state.sqlite");
    let app = App::open(db.to_str().unwrap(), root.clone()).unwrap();
    let parent = app
        .create_knot("Plan", None, Some("idea"), Some("default"))
        .unwrap();

    let ep = ExecutionPlanData {
        knot_ids: vec!["nonexistent".to_string()],
        ..Default::default()
    };

    let err = app
        .update_knot(&parent.id, patch(ep))
        .expect_err("should reject unknown id");

    match err {
        AppError::NotFound(msg) => {
            assert!(
                msg.contains("nonexistent"),
                "error should mention the bad id: {msg}",
            );
        }
        other => panic!("expected NotFound, got: {other:?}"),
    }

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn show_json_displays_qualified_ids_at_all_levels() {
    let root = workspace();
    let db = root.join(".knots/cache/state.sqlite");
    let app = App::open(db.to_str().unwrap(), root.clone()).unwrap();
    let parent = app
        .create_knot("Plan", None, Some("idea"), Some("default"))
        .unwrap();
    let child = app
        .create_knot("Task", None, Some("idea"), Some("default"))
        .unwrap();

    let bare = display_id(&child.id).to_string();
    let ep = ExecutionPlanData {
        knot_ids: vec![bare.clone()],
        waves: vec![ExecutionPlanWave {
            knots: vec![ExecutionPlanKnot {
                id: bare.clone(),
                title: "t".into(),
            }],
            steps: vec![ExecutionPlanStep {
                step_index: 1,
                knot_ids: vec![bare],
                notes: None,
            }],
            ..Default::default()
        }],
        ..Default::default()
    };

    app.update_knot(&parent.id, patch(ep))
        .expect("update should succeed");

    let shown = app.show_knot(&parent.id).expect("show").expect("exists");

    let plan = shown.execution_plan.expect("plan present");
    assert_eq!(plan.knot_ids, vec![child.id.clone()]);
    assert_eq!(plan.waves[0].knots[0].id, child.id);
    assert_eq!(plan.waves[0].steps[0].knot_ids, vec![child.id],);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn show_json_canonicalizes_legacy_bare_ids_from_cache() {
    let root = workspace();
    let db = root.join(".knots/cache/state.sqlite");
    let app = App::open(db.to_str().unwrap(), root.clone()).unwrap();
    let parent = app
        .create_knot("Plan", None, Some("idea"), Some("default"))
        .unwrap();
    let child = app
        .create_knot("Task", None, Some("idea"), Some("default"))
        .unwrap();

    let bare = display_id(&child.id).to_string();
    let ep = ExecutionPlanData {
        knot_ids: vec![bare.clone()],
        unassigned_knot_ids: vec![bare.clone()],
        waves: vec![ExecutionPlanWave {
            knots: vec![ExecutionPlanKnot {
                id: bare.clone(),
                title: "t".into(),
            }],
            steps: vec![ExecutionPlanStep {
                step_index: 1,
                knot_ids: vec![bare],
                notes: None,
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    let json = serde_json::to_string(&ep).expect("serialize");
    let conn = open_connection(db.to_str().unwrap()).expect("db");
    conn.execute(
        "UPDATE knot_hot SET execution_plan_data_json=?1 WHERE id=?2",
        params![json, parent.id],
    )
    .expect("rewrite hot cache");

    let shown = app.show_knot(&parent.id).expect("show").expect("exists");

    let plan = shown.execution_plan.expect("plan present");
    assert_eq!(plan.knot_ids, vec![child.id.clone()]);
    assert_eq!(plan.unassigned_knot_ids, vec![child.id.clone()]);
    assert_eq!(plan.waves[0].knots[0].id, child.id.clone());
    assert_eq!(plan.waves[0].steps[0].knot_ids, vec![child.id]);

    let _ = std::fs::remove_dir_all(root);
}
