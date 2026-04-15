use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Parser;

use crate::app::{App, AppError};
use crate::domain::execution_plan::{ExecutionPlanData, ExecutionPlanStep, ExecutionPlanWave};
use crate::write_queue::{
    PlanStepAddOperation, PlanStepMoveOperation, PlanStepRemoveOperation, PlanWaveAddOperation,
    PlanWaveMoveOperation, PlanWaveRemoveOperation, WriteOperation,
};

use super::{execute_operation, operation_from_command};

fn unique_workspace(prefix: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("temp workspace should be creatable");
    root
}

fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git").arg("-C").arg(root).args(args).output();
    let output = output.expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn setup_repo(root: &Path) {
    run_git(root, &["init"]);
    run_git(root, &["config", "user.email", "knots@example.com"]);
    run_git(root, &["config", "user.name", "Knots Test"]);
    std::fs::write(root.join("README.md"), "# knots\n").expect("readme should be writable");
    run_git(root, &["add", "README.md"]);
    run_git(root, &["commit", "-m", "init"]);
    run_git(root, &["branch", "-M", "main"]);
}

fn open_app(root: &Path) -> App {
    let db = root.join(".knots/cache/state.sqlite");
    App::open(db.to_str().expect("utf8"), root.to_path_buf()).expect("app should open")
}

fn seed_plan(app: &App, knot_id: &str, plan: ExecutionPlanData) {
    app.update_knot(
        knot_id,
        crate::app::types::UpdateKnotPatch {
            execution_plan_data: Some(plan),
            ..Default::default()
        },
    )
    .expect("plan should seed");
}

#[test]
fn operation_from_command_maps_plan_variants() {
    let wave_add = crate::cli::Cli::parse_from([
        "kno",
        "plan",
        "wave",
        "add",
        "knots-1",
        "--name",
        "Wave 1",
        "--objective",
        "Do the thing",
    ]);
    match operation_from_command(&wave_add.command).unwrap() {
        WriteOperation::PlanWaveAdd(op) => {
            assert_eq!(op.id, "knots-1");
            assert_eq!(op.name, "Wave 1");
            assert_eq!(op.objective, "Do the thing");
        }
        other => panic!("unexpected: {other:?}"),
    }

    let wave_remove = crate::cli::Cli::parse_from([
        "kno", "plan", "wave", "remove", "knots-1", "--wave", "2", "--force",
    ]);
    match operation_from_command(&wave_remove.command).unwrap() {
        WriteOperation::PlanWaveRemove(op) => {
            assert_eq!(op.wave, 2);
            assert!(op.force);
        }
        other => panic!("unexpected: {other:?}"),
    }

    let wave_move = crate::cli::Cli::parse_from([
        "kno", "plan", "wave", "move", "knots-1", "--from", "2", "--to", "1",
    ]);
    match operation_from_command(&wave_move.command).unwrap() {
        WriteOperation::PlanWaveMove(op) => {
            assert_eq!(op.from_index, 2);
            assert_eq!(op.to_index, 1);
        }
        other => panic!("unexpected: {other:?}"),
    }

    let step_add = crate::cli::Cli::parse_from([
        "kno",
        "plan",
        "step",
        "add",
        "knots-1",
        "--wave",
        "1",
        "--knot-ids",
        "knots-a,knots-b",
        "--notes",
        "land it",
    ]);
    match operation_from_command(&step_add.command).unwrap() {
        WriteOperation::PlanStepAdd(op) => {
            assert_eq!(op.wave, 1);
            assert_eq!(op.knot_ids, vec!["knots-a", "knots-b"]);
            assert_eq!(op.notes.as_deref(), Some("land it"));
        }
        other => panic!("unexpected: {other:?}"),
    }

    let step_remove = crate::cli::Cli::parse_from([
        "kno", "plan", "step", "remove", "knots-1", "--wave", "1", "--step", "3", "--force",
    ]);
    match operation_from_command(&step_remove.command).unwrap() {
        WriteOperation::PlanStepRemove(op) => {
            assert_eq!(op.wave, 1);
            assert_eq!(op.step, 3);
            assert!(op.force);
        }
        other => panic!("unexpected: {other:?}"),
    }

    let step_move = crate::cli::Cli::parse_from([
        "kno", "plan", "step", "move", "knots-1", "--wave", "1", "--from", "1", "--to", "2",
    ]);
    match operation_from_command(&step_move.command).unwrap() {
        WriteOperation::PlanStepMove(op) => {
            assert_eq!(op.wave, 1);
            assert_eq!(op.from_index, 1);
            assert_eq!(op.to_index, 2);
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn execute_operation_plan_wave_edits_persist() {
    let root = unique_workspace("knots-plan-wave-edit");
    setup_repo(&root);
    let app = open_app(&root);
    let target = app
        .create_knot("Plan carrier", None, Some("idea"), Some("default"))
        .expect("knot should create");

    let add_first = execute_operation(
        &app,
        &WriteOperation::PlanWaveAdd(PlanWaveAddOperation {
            id: target.id.clone(),
            name: "First".to_string(),
            objective: "Do first".to_string(),
            at: None,
        }),
    )
    .expect("wave add should succeed");
    assert!(add_first.contains("wave added"));

    let add_second = execute_operation(
        &app,
        &WriteOperation::PlanWaveAdd(PlanWaveAddOperation {
            id: target.id.clone(),
            name: "Second".to_string(),
            objective: "Do second".to_string(),
            at: None,
        }),
    )
    .expect("second wave add should succeed");
    assert!(add_second.contains("wave added"));

    let moved = execute_operation(
        &app,
        &WriteOperation::PlanWaveMove(PlanWaveMoveOperation {
            id: target.id.clone(),
            from_index: 2,
            to_index: 1,
        }),
    )
    .expect("wave move should succeed");
    assert!(moved.contains("wave moved"));

    let updated = app.show_knot(&target.id).expect("show").expect("knot");
    let plan = updated.execution_plan.expect("plan should exist");
    assert_eq!(plan.waves.len(), 2);
    assert_eq!(plan.waves[0].name, "Second");
    assert_eq!(plan.waves[1].name, "First");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn execute_operation_plan_step_edits_persist() {
    let root = unique_workspace("knots-plan-step-edit");
    setup_repo(&root);
    let app = open_app(&root);
    let target = app
        .create_knot("Plan carrier", None, Some("idea"), Some("default"))
        .expect("knot should create");
    let ref_a = app
        .create_knot("Ref A", None, Some("idea"), Some("default"))
        .expect("ref a");
    let ref_b = app
        .create_knot("Ref B", None, Some("idea"), Some("default"))
        .expect("ref b");

    seed_plan(
        &app,
        &target.id,
        ExecutionPlanData {
            waves: vec![ExecutionPlanWave {
                wave_index: 1,
                name: "Wave".to_string(),
                objective: "Objective".to_string(),
                steps: vec![ExecutionPlanStep {
                    step_index: 1,
                    knot_ids: vec![ref_a.id.clone()],
                    notes: Some("first".to_string()),
                }],
                ..Default::default()
            }],
            ..Default::default()
        },
    );

    let added = execute_operation(
        &app,
        &WriteOperation::PlanStepAdd(PlanStepAddOperation {
            id: target.id.clone(),
            wave: 1,
            knot_ids: vec![ref_b.id.clone()],
            notes: Some("second".to_string()),
            at: Some(2),
        }),
    )
    .expect("step add should succeed");
    assert!(added.contains("step added"));

    let moved = execute_operation(
        &app,
        &WriteOperation::PlanStepMove(PlanStepMoveOperation {
            id: target.id.clone(),
            wave: 1,
            from_index: 2,
            to_index: 1,
        }),
    )
    .expect("step move should succeed");
    assert!(moved.contains("step moved"));

    let removed = execute_operation(
        &app,
        &WriteOperation::PlanStepRemove(PlanStepRemoveOperation {
            id: target.id.clone(),
            wave: 1,
            step: 2,
            force: true,
        }),
    )
    .expect("step remove should succeed");
    assert!(removed.contains("step removed"));

    let updated = app.show_knot(&target.id).expect("show").expect("knot");
    let plan = updated.execution_plan.expect("plan should exist");
    assert_eq!(plan.waves[0].steps.len(), 1);
    assert_eq!(plan.waves[0].steps[0].knot_ids, vec![ref_b.id.clone()]);
    assert_eq!(plan.waves[0].steps[0].step_index, 1);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn execute_operation_plan_remove_force_skips_confirmation() {
    let root = unique_workspace("knots-plan-remove-force");
    setup_repo(&root);
    let app = open_app(&root);
    let target = app
        .create_knot("Plan carrier", None, Some("idea"), Some("default"))
        .expect("knot should create");
    let ref_a = app
        .create_knot("Ref A", None, Some("idea"), Some("default"))
        .expect("ref a");
    seed_plan(
        &app,
        &target.id,
        ExecutionPlanData {
            waves: vec![
                ExecutionPlanWave {
                    wave_index: 1,
                    name: "Keep".to_string(),
                    objective: "Keep".to_string(),
                    ..Default::default()
                },
                ExecutionPlanWave {
                    wave_index: 2,
                    name: "Drop".to_string(),
                    objective: "Drop".to_string(),
                    knots: vec![crate::domain::execution_plan::ExecutionPlanKnot {
                        id: ref_a.id.clone(),
                        title: "Ref A".to_string(),
                    }],
                    steps: vec![ExecutionPlanStep {
                        step_index: 1,
                        knot_ids: vec![ref_a.id.clone()],
                        notes: None,
                    }],
                    ..Default::default()
                },
            ],
            ..Default::default()
        },
    );

    let output = execute_operation(
        &app,
        &WriteOperation::PlanWaveRemove(PlanWaveRemoveOperation {
            id: target.id.clone(),
            wave: 2,
            force: true,
        }),
    )
    .expect("wave remove should succeed");
    assert!(output.contains("wave removed"));

    let updated = app.show_knot(&target.id).expect("show").expect("knot");
    let plan = updated.execution_plan.expect("plan should exist");
    assert_eq!(plan.waves.len(), 1);
    assert_eq!(plan.waves[0].wave_index, 1);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn execute_operation_plan_wave_remove_without_force_rejects_non_tty() {
    let root = unique_workspace("knots-plan-wave-remove-non-tty");
    setup_repo(&root);
    let app = open_app(&root);
    let target = app
        .create_knot("Plan carrier", None, Some("idea"), Some("default"))
        .expect("knot should create");
    let ref_a = app
        .create_knot("Ref A", None, Some("idea"), Some("default"))
        .expect("ref a");
    seed_plan(
        &app,
        &target.id,
        ExecutionPlanData {
            waves: vec![ExecutionPlanWave {
                wave_index: 1,
                name: "Drop".to_string(),
                objective: "Drop".to_string(),
                knots: vec![crate::domain::execution_plan::ExecutionPlanKnot {
                    id: ref_a.id.clone(),
                    title: "Ref A".to_string(),
                }],
                steps: vec![ExecutionPlanStep {
                    step_index: 1,
                    knot_ids: vec![ref_a.id.clone()],
                    notes: None,
                }],
                ..Default::default()
            }],
            ..Default::default()
        },
    );

    let err = execute_operation(
        &app,
        &WriteOperation::PlanWaveRemove(PlanWaveRemoveOperation {
            id: target.id.clone(),
            wave: 1,
            force: false,
        }),
    )
    .expect_err("wave remove should require tty");
    assert!(matches!(err, AppError::InvalidArgument(_)));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn execute_operation_plan_step_remove_without_force_rejects_non_tty() {
    let root = unique_workspace("knots-plan-step-remove-non-tty");
    setup_repo(&root);
    let app = open_app(&root);
    let target = app
        .create_knot("Plan carrier", None, Some("idea"), Some("default"))
        .expect("knot should create");
    let ref_a = app
        .create_knot("Ref A", None, Some("idea"), Some("default"))
        .expect("ref a");
    seed_plan(
        &app,
        &target.id,
        ExecutionPlanData {
            waves: vec![ExecutionPlanWave {
                wave_index: 1,
                name: "Drop".to_string(),
                objective: "Drop".to_string(),
                steps: vec![ExecutionPlanStep {
                    step_index: 1,
                    knot_ids: vec![ref_a.id.clone()],
                    notes: None,
                }],
                ..Default::default()
            }],
            ..Default::default()
        },
    );

    let err = execute_operation(
        &app,
        &WriteOperation::PlanStepRemove(PlanStepRemoveOperation {
            id: target.id.clone(),
            wave: 1,
            step: 1,
            force: false,
        }),
    )
    .expect_err("step remove should require tty");
    assert!(matches!(err, AppError::InvalidArgument(_)));

    let _ = std::fs::remove_dir_all(root);
}
