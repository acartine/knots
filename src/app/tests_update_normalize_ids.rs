use super::{App, AppError, UpdateKnotPatch};
use crate::db::open_connection;
use crate::domain::execution_plan::{
    ExecutionPlanData, ExecutionPlanKnot, ExecutionPlanStep, ExecutionPlanWave,
};
use crate::knot_id::display_id;
use rusqlite::params;
use serde_json::json;

fn workspace() -> knots_test_support::TestWorkspace {
    let r_ws = knots_test_support::workspace("knots-norm-test");
    let _r = r_ws.path().to_path_buf();
    r_ws
}

fn patch(ep: ExecutionPlanData) -> UpdateKnotPatch {
    UpdateKnotPatch {
        execution_plan_data: Some(ep),
        ..Default::default()
    }
}

#[test]
fn bare_ids_are_normalized_to_fully_qualified() {
    let root_ws = workspace();
    let root = root_ws.path().to_path_buf();
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
    assert_eq!(plan.unassigned_knot_ids, vec![child.id.clone()],);
    assert_eq!(plan.waves[0].knots[0].id, child.id);
    assert_eq!(plan.waves[0].steps[0].knot_ids, vec![child.id.clone()],);
}

#[test]
fn already_qualified_ids_pass_through_unchanged() {
    let root_ws = workspace();
    let root = root_ws.path().to_path_buf();
    let db = root.join(".knots/cache/state.sqlite");
    let app = App::open(db.to_str().unwrap(), root.clone()).unwrap();
    let parent = app
        .create_knot("Plan", None, Some("idea"), Some("default"))
        .unwrap();
    let child = app
        .create_knot("Task", None, Some("idea"), Some("default"))
        .unwrap();

    let ep = ExecutionPlanData {
        unassigned_knot_ids: vec![child.id.clone()],
        ..Default::default()
    };

    let updated = app
        .update_knot(&parent.id, patch(ep))
        .expect("update should succeed");

    let plan = updated.execution_plan.expect("plan present");
    assert_eq!(plan.unassigned_knot_ids, vec![child.id]);
}

#[test]
fn unresolvable_ids_are_rejected() {
    let root_ws = workspace();
    let root = root_ws.path().to_path_buf();
    let db = root.join(".knots/cache/state.sqlite");
    let app = App::open(db.to_str().unwrap(), root.clone()).unwrap();
    let parent = app
        .create_knot("Plan", None, Some("idea"), Some("default"))
        .unwrap();

    let ep = ExecutionPlanData {
        unassigned_knot_ids: vec!["nonexistent".to_string()],
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
}

#[test]
fn show_json_displays_qualified_ids_at_all_levels() {
    let root_ws = workspace();
    let root = root_ws.path().to_path_buf();
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

    app.update_knot(&parent.id, patch(ep))
        .expect("update should succeed");

    let shown = app.show_knot(&parent.id).expect("show").expect("exists");

    let plan = shown.execution_plan.expect("plan present");
    assert_eq!(plan.unassigned_knot_ids, vec![child.id.clone()]);
    assert_eq!(plan.waves[0].knots[0].id, child.id);
    assert_eq!(plan.waves[0].steps[0].knot_ids, vec![child.id],);
}

#[test]
fn show_json_canonicalizes_legacy_bare_ids_from_cache() {
    let root_ws = workspace();
    let root = root_ws.path().to_path_buf();
    let db = root.join(".knots/cache/state.sqlite");
    let app = App::open(db.to_str().unwrap(), root.clone()).unwrap();
    let parent = app
        .create_knot("Plan", None, Some("idea"), Some("default"))
        .unwrap();
    let child = app
        .create_knot("Task", None, Some("idea"), Some("default"))
        .unwrap();

    let bare = display_id(&child.id).to_string();
    let payload = json!({
        "repo_path": "/repo",
        "knot_ids": [bare.clone()],
        "unassigned_knot_ids": [bare.clone()],
        "waves": [{
            "wave_index": 1,
            "knots": [{
                "id": bare.clone(),
                "title": "t"
            }],
            "steps": [{
                "step_index": 1,
                "knot_ids": [bare]
            }]
        }]
    });
    let conn = open_connection(db.to_str().unwrap()).expect("db");
    conn.execute(
        "UPDATE knot_hot SET execution_plan_data_json=?1 WHERE id=?2",
        params![payload.to_string(), parent.id],
    )
    .expect("rewrite hot cache");

    let shown = app.show_knot(&parent.id).expect("show").expect("exists");

    let plan = shown.execution_plan.expect("plan present");
    assert_eq!(plan.unassigned_knot_ids, vec![child.id.clone()]);
    assert_eq!(plan.waves[0].knots[0].id, child.id.clone());
    assert_eq!(plan.waves[0].steps[0].knot_ids, vec![child.id]);
}
