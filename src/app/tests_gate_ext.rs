use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use super::{
    App, AppError, CreateKnotOptions, GateDecision, KnotView, StateActorMetadata, UpdateKnotPatch,
};
use crate::domain::gate::{GateData, GateOwnerKind};
use crate::domain::invariant::{Invariant, InvariantType};
use crate::domain::knot_type::KnotType;

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-app-gate-ext-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    root
}

fn open_app(root: &Path) -> App {
    let db_path = root.join(".knots/cache/state.sqlite");
    App::open(db_path.to_str().unwrap(), root.to_path_buf()).expect("app should open")
}

fn add_invariants(app: &App, knot: &KnotView, invariants: Vec<Invariant>) -> KnotView {
    app.update_knot(
        &knot.id,
        UpdateKnotPatch {
            add_invariants: invariants,
            expected_profile_etag: knot.profile_etag.clone(),
            ..UpdateKnotPatch::default()
        },
    )
    .expect("invariants should be added")
}

#[test]
fn evaluate_gate_yes_ships_without_reopening() {
    let root = unique_workspace();
    let app = open_app(&root);
    let gate = app
        .create_knot_with_options(
            "Ship gate",
            None,
            None,
            Some("default"),
            CreateKnotOptions {
                knot_type: KnotType::Gate,
                gate_data: GateData::default(),
            },
        )
        .expect("gate should be created");
    let gate = app
        .set_state(
            &gate.id,
            crate::workflow_runtime::EVALUATING,
            false,
            gate.profile_etag.as_deref(),
        )
        .expect("gate should enter evaluating");

    let result = app
        .evaluate_gate(
            &gate.id,
            GateDecision::Yes,
            None,
            StateActorMetadata::default(),
        )
        .expect("gate should evaluate");

    assert_eq!(result.decision, "yes");
    assert_eq!(result.gate.state, "shipped");
    assert!(result.reopened.is_empty());
    assert!(result.invariant.is_none());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn evaluate_gate_no_covers_missing_invariant_and_failure_mode_errors() {
    let root = unique_workspace();
    let app = open_app(&root);
    let gate = app
        .create_knot_with_options(
            "Release gate",
            None,
            None,
            Some("default"),
            CreateKnotOptions {
                knot_type: KnotType::Gate,
                gate_data: GateData::default(),
            },
        )
        .expect("gate should be created");
    let gate = add_invariants(
        &app,
        &gate,
        vec![Invariant::new(InvariantType::State, "release blocked")
            .expect("invariant should build")],
    );
    let gate = app
        .set_state(
            &gate.id,
            crate::workflow_runtime::EVALUATING,
            false,
            gate.profile_etag.as_deref(),
        )
        .expect("gate should enter evaluating");

    let missing_invariant = app
        .evaluate_gate(
            &gate.id,
            GateDecision::No,
            None,
            StateActorMetadata::default(),
        )
        .expect_err("missing invariant should fail");
    assert!(matches!(missing_invariant, AppError::InvalidArgument(_)));
    assert!(missing_invariant
        .to_string()
        .contains("--invariant is required"));

    let undefined_invariant = app
        .evaluate_gate(
            &gate.id,
            GateDecision::No,
            Some("missing invariant"),
            StateActorMetadata::default(),
        )
        .expect_err("unknown invariant should fail");
    assert!(undefined_invariant
        .to_string()
        .contains("does not define invariant"));

    let missing_failure_mode = app
        .evaluate_gate(
            &gate.id,
            GateDecision::No,
            Some("release blocked"),
            StateActorMetadata::default(),
        )
        .expect_err("missing failure mode should fail");
    assert!(missing_failure_mode
        .to_string()
        .contains("has no failure mode"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn gate_metadata_updates_round_trip_and_work_knots_reject_them() {
    let root = unique_workspace();
    let app = open_app(&root);
    let target = app
        .create_knot("Blocked work", None, Some("idea"), Some("default"))
        .expect("target should be created");
    let gate = app
        .create_knot_with_options(
            "Metadata gate",
            None,
            None,
            Some("default"),
            CreateKnotOptions {
                knot_type: KnotType::Gate,
                gate_data: GateData::default(),
            },
        )
        .expect("gate should be created");

    let mut failure_modes = BTreeMap::new();
    failure_modes.insert("release blocked".to_string(), vec![target.id.clone()]);
    let updated = app
        .update_knot(
            &gate.id,
            UpdateKnotPatch {
                gate_owner_kind: Some(GateOwnerKind::Human),
                gate_failure_modes: Some(failure_modes),
                expected_profile_etag: gate.profile_etag.clone(),
                ..Default::default()
            },
        )
        .expect("gate metadata should update");
    assert_eq!(
        updated.gate.as_ref().map(|g| g.owner_kind),
        Some(GateOwnerKind::Human)
    );
    assert_eq!(
        updated
            .gate
            .as_ref()
            .and_then(|g| g.failure_modes.get("release blocked"))
            .cloned(),
        Some(vec![target.id.clone()])
    );

    let cleared = app
        .update_knot(
            &gate.id,
            UpdateKnotPatch {
                clear_gate_failure_modes: true,
                expected_profile_etag: updated.profile_etag.clone(),
                ..Default::default()
            },
        )
        .expect("gate failure modes should clear");
    assert!(cleared
        .gate
        .expect("gate metadata should exist")
        .failure_modes
        .is_empty());

    let work = app
        .create_knot("Work knot", None, Some("idea"), Some("default"))
        .expect("work knot should be created");
    let err = app
        .update_knot(
            &work.id,
            UpdateKnotPatch {
                gate_owner_kind: Some(GateOwnerKind::Human),
                expected_profile_etag: work.profile_etag.clone(),
                ..Default::default()
            },
        )
        .expect_err("work knot should reject gate metadata");
    assert!(err.to_string().contains("require knot type 'gate'"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn evaluate_gate_no_reopens_targets_and_appends_metadata() {
    let root = unique_workspace();
    let app = open_app(&root);

    let target_a = app
        .create_knot("Work A", None, Some("implementation"), Some("default"))
        .expect("target A should be created");
    let target_b = app
        .create_knot("Work B", None, Some("shipped"), Some("default"))
        .expect("target B should be created");

    let mut failure_modes = BTreeMap::new();
    failure_modes.insert(
        "tests must pass".to_string(),
        vec![target_a.id.clone(), target_b.id.clone()],
    );
    let gate = app
        .create_knot_with_options(
            "Quality gate",
            None,
            None,
            Some("default"),
            CreateKnotOptions {
                knot_type: KnotType::Gate,
                gate_data: GateData {
                    owner_kind: GateOwnerKind::Agent,
                    failure_modes,
                },
            },
        )
        .expect("gate should be created");
    let gate = add_invariants(
        &app,
        &gate,
        vec![Invariant::new(InvariantType::State, "tests must pass")
            .expect("invariant should build")],
    );
    let gate = app
        .set_state(
            &gate.id,
            crate::workflow_runtime::EVALUATING,
            false,
            gate.profile_etag.as_deref(),
        )
        .expect("gate should enter evaluating");

    let result = app
        .evaluate_gate(
            &gate.id,
            GateDecision::No,
            Some("tests must pass"),
            StateActorMetadata::default(),
        )
        .expect("gate evaluation should succeed");

    assert_eq!(result.decision, "no");
    assert_eq!(result.gate.state, "abandoned");
    assert_eq!(result.invariant.as_deref(), Some("tests must pass"));
    assert_eq!(result.reopened.len(), 2);
    assert!(result.reopened.contains(&target_a.id));
    assert!(result.reopened.contains(&target_b.id));

    let reloaded_a = app
        .show_knot(&target_a.id)
        .expect("show should succeed")
        .expect("target A should exist");
    assert_eq!(reloaded_a.state, "ready_for_planning");
    assert_eq!(reloaded_a.notes.len(), 1);
    assert!(reloaded_a.notes[0].content.contains("failed invariant"));
    assert!(reloaded_a.notes[0].content.contains(&gate.id));
    assert_eq!(reloaded_a.handoff_capsules.len(), 1);
    assert!(reloaded_a.handoff_capsules[0]
        .content
        .contains("reopened this knot for planning"));

    let reloaded_b = app
        .show_knot(&target_b.id)
        .expect("show should succeed")
        .expect("target B should exist");
    assert_eq!(reloaded_b.state, "ready_for_planning");
    assert_eq!(reloaded_b.notes.len(), 1);
    assert_eq!(reloaded_b.handoff_capsules.len(), 1);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn evaluate_gate_no_skips_transition_for_already_ready_target() {
    let root = unique_workspace();
    let app = open_app(&root);

    let target = app
        .create_knot(
            "Already ready",
            None,
            Some("ready_for_planning"),
            Some("default"),
        )
        .expect("target should be created");

    let mut failure_modes = BTreeMap::new();
    failure_modes.insert("scope unchanged".to_string(), vec![target.id.clone()]);
    let gate = app
        .create_knot_with_options(
            "Scope gate",
            None,
            None,
            Some("default"),
            CreateKnotOptions {
                knot_type: KnotType::Gate,
                gate_data: GateData {
                    owner_kind: GateOwnerKind::Agent,
                    failure_modes,
                },
            },
        )
        .expect("gate should be created");
    let gate = add_invariants(
        &app,
        &gate,
        vec![Invariant::new(InvariantType::Scope, "scope unchanged")
            .expect("invariant should build")],
    );
    let gate = app
        .set_state(
            &gate.id,
            crate::workflow_runtime::EVALUATING,
            false,
            gate.profile_etag.as_deref(),
        )
        .expect("gate should enter evaluating");

    let result = app
        .evaluate_gate(
            &gate.id,
            GateDecision::No,
            Some("scope unchanged"),
            StateActorMetadata::default(),
        )
        .expect("gate evaluation should succeed");

    assert_eq!(result.decision, "no");
    assert_eq!(result.gate.state, "abandoned");
    assert_eq!(result.reopened, vec![target.id.clone()]);

    let reloaded = app
        .show_knot(&target.id)
        .expect("show should succeed")
        .expect("target should exist");
    assert_eq!(reloaded.state, "ready_for_planning");
    assert_eq!(reloaded.notes.len(), 1);
    assert!(reloaded.notes[0].content.contains(&gate.id));
    assert_eq!(reloaded.handoff_capsules.len(), 1);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn evaluate_gate_rejects_non_gate_and_wrong_state() {
    let root = unique_workspace();
    let app = open_app(&root);

    let work = app
        .create_knot("Regular work", None, Some("idea"), Some("default"))
        .expect("work knot should be created");
    let err = app
        .evaluate_gate(
            &work.id,
            GateDecision::Yes,
            None,
            StateActorMetadata::default(),
        )
        .expect_err("non-gate should fail");
    assert!(err.to_string().contains("is not a gate"));

    let gate = app
        .create_knot_with_options(
            "Unevaluated gate",
            None,
            None,
            Some("default"),
            CreateKnotOptions {
                knot_type: KnotType::Gate,
                gate_data: GateData::default(),
            },
        )
        .expect("gate should be created");
    let err = app
        .evaluate_gate(
            &gate.id,
            GateDecision::Yes,
            None,
            StateActorMetadata::default(),
        )
        .expect_err("gate not in evaluating should fail");
    assert!(err.to_string().contains("must be in"));

    let _ = std::fs::remove_dir_all(root);
}
