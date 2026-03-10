use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use super::{App, AppError, CreateKnotOptions, GateDecision, StateActorMetadata, UpdateKnotPatch};
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
    App::open(db_path.to_str().expect("utf8 db path"), root.to_path_buf()).expect("app should open")
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
    let gate = app
        .update_knot(
            &gate.id,
            UpdateKnotPatch {
                title: None,
                description: None,
                priority: None,
                status: None,
                knot_type: None,
                add_tags: vec![],
                remove_tags: vec![],
                add_invariants: vec![Invariant::new(InvariantType::State, "release blocked")
                    .expect("invariant should build")],
                remove_invariants: vec![],
                clear_invariants: false,
                gate_owner_kind: None,
                gate_failure_modes: None,
                clear_gate_failure_modes: false,
                add_note: None,
                add_handoff_capsule: None,
                expected_profile_etag: gate.profile_etag.clone(),
                force: false,
                state_actor: StateActorMetadata::default(),
            },
        )
        .expect("gate should update");
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
                title: None,
                description: None,
                priority: None,
                status: None,
                knot_type: None,
                add_tags: vec![],
                remove_tags: vec![],
                add_invariants: vec![],
                remove_invariants: vec![],
                clear_invariants: false,
                gate_owner_kind: Some(GateOwnerKind::Human),
                gate_failure_modes: Some(failure_modes),
                clear_gate_failure_modes: false,
                add_note: None,
                add_handoff_capsule: None,
                expected_profile_etag: gate.profile_etag.clone(),
                force: false,
                state_actor: StateActorMetadata::default(),
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
                title: None,
                description: None,
                priority: None,
                status: None,
                knot_type: None,
                add_tags: vec![],
                remove_tags: vec![],
                add_invariants: vec![],
                remove_invariants: vec![],
                clear_invariants: false,
                gate_owner_kind: None,
                gate_failure_modes: None,
                clear_gate_failure_modes: true,
                add_note: None,
                add_handoff_capsule: None,
                expected_profile_etag: updated.profile_etag.clone(),
                force: false,
                state_actor: StateActorMetadata::default(),
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
                title: None,
                description: None,
                priority: None,
                status: None,
                knot_type: None,
                add_tags: vec![],
                remove_tags: vec![],
                add_invariants: vec![],
                remove_invariants: vec![],
                clear_invariants: false,
                gate_owner_kind: Some(GateOwnerKind::Human),
                gate_failure_modes: None,
                clear_gate_failure_modes: false,
                add_note: None,
                add_handoff_capsule: None,
                expected_profile_etag: work.profile_etag.clone(),
                force: false,
                state_actor: StateActorMetadata::default(),
            },
        )
        .expect_err("work knot should reject gate metadata");
    assert!(err.to_string().contains("require knot type 'gate'"));

    let _ = std::fs::remove_dir_all(root);
}
