//! Tests covering the lease-declared agent-identity contract.
//!
//! The lease is the single declared source of agent identity for a claim.
//! Per-command `--agent-*` / `--note-*` / `--handoff-*` flags on every `kno`
//! subcommand except `kno lease create` are deprecated: syntactically
//! accepted, runtime-ignored, and trigger a warning. Knots stamps identity
//! onto notes, handoff capsules, step-history entries, and gate decisions
//! from the bound lease's `agent_info`. This module verifies each sink.

use super::execute_operation;
use super::tests_lease_ext::{create_test_lease, open_app, setup_repo, unique_workspace};
use crate::poll_claim;
use crate::write_queue::{
    GateEvaluateOperation, NextOperation, RollbackOperation, StepAnnotateOperation,
    UpdateOperation, WriteOperation,
};

// create_test_lease creates a lease with:
//   agent_type: "cli", provider: "Anthropic", agent_name: "claude",
//   model: "opus", model_version: "4.6"

fn claim_with_lease(app: &crate::app::App, knot_id: &str, lease_id: &str) -> String {
    let claimed = poll_claim::claim_knot(
        app,
        knot_id,
        Some("agent".to_string()),
        Some(lease_id),
        600,
        false,
    )
    .expect("claim with lease should succeed");
    claimed.knot.state
}

fn base_update_op(id: &str) -> UpdateOperation {
    UpdateOperation {
        id: id.to_string(),
        title: None,
        description: None,
        acceptance: None,
        priority: None,
        status: None,
        knot_type: None,
        add_tags: vec![],
        remove_tags: vec![],
        add_invariants: vec![],
        remove_invariants: vec![],
        clear_invariants: false,
        add_verification_steps: vec![],
        remove_verification_steps: vec![],
        clear_verification_steps: false,
        gate_owner_kind: None,
        gate_failure_modes: vec![],
        clear_gate_failure_modes: false,
        scope: crate::cli_scope::ScopeArgs::default(),
        execution_plan_file: None,
        objective: None,
        add_note: None,
        note_username: None,
        note_datetime: None,
        note_agentname: None,
        note_model: None,
        note_version: None,
        add_handoff_capsule: None,
        handoff_username: None,
        handoff_datetime: None,
        handoff_agentname: None,
        handoff_model: None,
        handoff_version: None,
        if_match: None,
        actor_kind: None,
        agent_name: None,
        agent_model: None,
        agent_version: None,
        force: false,
        approve_terminal_cascade: false,
        lease_id: None,
        json: false,
    }
}

#[test]
fn next_stamps_lease_agent_identity_on_step_history() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let app = open_app(&root);

    let knot = app
        .create_knot("Lease next identity", None, Some("work_item"), None)
        .expect("create knot");
    let lease_id = create_test_lease(&app);
    let _ = claim_with_lease(&app, &knot.id, &lease_id);

    execute_operation(
        &app,
        &WriteOperation::Next(NextOperation {
            id: knot.id.clone(),
            expected_state: None,
            json: false,
            approve_terminal_cascade: false,
            actor_kind: Some("agent".to_string()),
            agent_name: None,
            agent_model: None,
            agent_version: None,
            lease_id: Some(lease_id.clone()),
        }),
    )
    .expect("next should succeed");

    let view = app.show_knot(&knot.id).expect("show").expect("knot");
    let step = view
        .step_history
        .iter()
        .find(|s| s.agent_name.is_some())
        .expect("at least one step should carry lease-sourced agent identity");
    assert_eq!(step.agent_name.as_deref(), Some("claude"));
    assert_eq!(step.agent_model.as_deref(), Some("opus"));
    assert_eq!(step.agent_version.as_deref(), Some("4.6"));
}

#[test]
fn next_ignores_bogus_agent_flags_and_uses_lease() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let app = open_app(&root);

    let knot = app
        .create_knot("Bogus flags next", None, Some("work_item"), None)
        .expect("create");
    let lease_id = create_test_lease(&app);
    let _ = claim_with_lease(&app, &knot.id, &lease_id);

    execute_operation(
        &app,
        &WriteOperation::Next(NextOperation {
            id: knot.id.clone(),
            expected_state: None,
            json: false,
            approve_terminal_cascade: false,
            actor_kind: Some("agent".to_string()),
            agent_name: Some("bogus".to_string()),
            agent_model: Some("bogus".to_string()),
            agent_version: Some("bogus".to_string()),
            lease_id: Some(lease_id.clone()),
        }),
    )
    .expect("next with bogus flags should still succeed (flags are ignored)");

    let view = app.show_knot(&knot.id).expect("show").expect("knot");
    for step in &view.step_history {
        if let Some(name) = step.agent_name.as_deref() {
            assert_eq!(name, "claude", "bogus --agent-name must be ignored");
        }
        if let Some(model) = step.agent_model.as_deref() {
            assert_eq!(model, "opus", "bogus --agent-model must be ignored");
        }
        if let Some(version) = step.agent_version.as_deref() {
            assert_eq!(version, "4.6", "bogus --agent-version must be ignored");
        }
    }
}

#[test]
fn next_without_lease_leaves_agent_fields_unset() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let app = open_app(&root);

    let knot = app
        .create_knot("No lease next", None, Some("work_item"), None)
        .expect("create");
    // Advance into an action state without claiming a lease so the next
    // transition has a step to record. The actor here has no agent
    // identity because there is no lease to source it from.
    app.set_state_with_actor(
        &knot.id,
        "implementation",
        false,
        None,
        crate::app::StateActorMetadata {
            actor_kind: Some("agent".to_string()),
            ..Default::default()
        },
    )
    .expect("advance to implementation");

    execute_operation(
        &app,
        &WriteOperation::Next(NextOperation {
            id: knot.id.clone(),
            expected_state: None,
            json: false,
            approve_terminal_cascade: false,
            actor_kind: Some("agent".to_string()),
            agent_name: None,
            agent_model: None,
            agent_version: None,
            lease_id: None,
        }),
    )
    .expect("next on unleased knot should succeed");

    let view = app.show_knot(&knot.id).expect("show").expect("knot");
    for step in &view.step_history {
        assert!(
            step.agent_name.is_none(),
            "agent_name must be unset when no lease is bound"
        );
        assert!(
            step.agent_model.is_none(),
            "agent_model must be unset when no lease is bound"
        );
        assert!(
            step.agent_version.is_none(),
            "agent_version must be unset when no lease is bound"
        );
    }
}

#[test]
fn update_add_note_uses_lease_not_caller_supplied_note_flags() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let app = open_app(&root);

    let knot = app
        .create_knot("Note lease identity", None, None, None)
        .expect("create");
    let lease_id = create_test_lease(&app);
    crate::lease::bind_lease(&app, &knot.id, &lease_id).expect("bind");

    let mut op = base_update_op(&knot.id);
    op.add_note = Some("lease-sourced note".to_string());
    // Supplied values must be ignored; lease wins.
    op.note_agentname = Some("bogus".to_string());
    op.note_model = Some("bogus".to_string());
    op.note_version = Some("bogus".to_string());
    execute_operation(&app, &WriteOperation::Update(op)).expect("update note");

    let view = app.show_knot(&knot.id).expect("show").expect("knot");
    let note = view.notes.last().expect("note should be recorded");
    assert_eq!(note.agentname, "claude");
    assert_eq!(note.model, "opus");
    assert_eq!(note.version, "4.6");
}

#[test]
fn update_add_handoff_uses_lease_not_caller_supplied_handoff_flags() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let app = open_app(&root);

    let knot = app
        .create_knot("Handoff lease identity", None, None, None)
        .expect("create");
    let lease_id = create_test_lease(&app);
    crate::lease::bind_lease(&app, &knot.id, &lease_id).expect("bind");

    let mut op = base_update_op(&knot.id);
    op.add_handoff_capsule = Some("lease-sourced handoff".to_string());
    op.handoff_agentname = Some("bogus".to_string());
    op.handoff_model = Some("bogus".to_string());
    op.handoff_version = Some("bogus".to_string());
    execute_operation(&app, &WriteOperation::Update(op)).expect("update handoff");

    let view = app.show_knot(&knot.id).expect("show").expect("knot");
    let hc = view.handoff_capsules.last().expect("handoff recorded");
    assert_eq!(hc.agentname, "claude");
    assert_eq!(hc.model, "opus");
    assert_eq!(hc.version, "4.6");
}

#[test]
fn rollback_ignores_bogus_agent_flags_and_uses_lease() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let app = open_app(&root);

    let knot = app
        .create_knot("Rollback lease identity", None, Some("work_item"), None)
        .expect("create");
    let lease_id = create_test_lease(&app);
    let _ = claim_with_lease(&app, &knot.id, &lease_id);

    execute_operation(
        &app,
        &WriteOperation::Rollback(RollbackOperation {
            id: knot.id.clone(),
            dry_run: false,
            actor_kind: Some("agent".to_string()),
            agent_name: Some("bogus".to_string()),
            agent_model: Some("bogus".to_string()),
            agent_version: Some("bogus".to_string()),
            json: false,
            outcome: None,
            lease_id: None,
        }),
    )
    .expect("rollback should succeed");

    let view = app.show_knot(&knot.id).expect("show").expect("knot");
    for step in &view.step_history {
        if let Some(name) = step.agent_name.as_deref() {
            assert_eq!(name, "claude");
        }
    }
}

#[test]
fn step_annotate_uses_lease_not_caller_supplied_agent_flags() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let app = open_app(&root);

    let knot = app
        .create_knot("Step annotate lease", None, Some("work_item"), None)
        .expect("create");
    let lease_id = create_test_lease(&app);
    let _ = claim_with_lease(&app, &knot.id, &lease_id);

    execute_operation(
        &app,
        &WriteOperation::StepAnnotate(StepAnnotateOperation {
            id: knot.id.clone(),
            actor_kind: Some("agent".to_string()),
            agent_name: Some("bogus".to_string()),
            agent_model: Some("bogus".to_string()),
            agent_version: Some("bogus".to_string()),
            json: false,
        }),
    )
    .expect("step annotate should succeed");

    let view = app.show_knot(&knot.id).expect("show").expect("knot");
    let step = view
        .step_history
        .last()
        .expect("at least one step after annotate");
    // Annotated step identity should come from the bound lease.
    assert_eq!(step.agent_name.as_deref(), Some("claude"));
    assert_eq!(step.agent_model.as_deref(), Some("opus"));
    assert_eq!(step.agent_version.as_deref(), Some("4.6"));
}

#[test]
fn gate_evaluate_uses_lease_not_caller_supplied_agent_flags() {
    use crate::app::StateActorMetadata;
    use crate::app::{CreateKnotOptions, UpdateKnotPatch};
    use crate::domain::gate::{GateData, GateOwnerKind};
    use crate::domain::invariant::{Invariant, InvariantType};
    use crate::domain::knot_type::KnotType;
    use std::collections::BTreeMap;

    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let app = open_app(&root);

    // Create a gate knot with an invariant. No failure modes are needed for
    // a "yes" decision; the gate ships and no reopen targets are consulted.
    let invariant = Invariant::new(InvariantType::State, "tests pass").expect("invariant");
    let failure_modes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let gate = app
        .create_knot_with_options(
            "Gate lease identity",
            None,
            None,
            None,
            None,
            CreateKnotOptions {
                knot_type: KnotType::Gate,
                gate_data: GateData {
                    owner_kind: GateOwnerKind::Agent,
                    failure_modes,
                },
                ..CreateKnotOptions::default()
            },
        )
        .expect("create gate");
    app.update_knot(
        &gate.id,
        UpdateKnotPatch {
            add_invariants: vec![invariant.clone()],
            expected_profile_etag: gate.profile_etag.clone(),
            ..UpdateKnotPatch::default()
        },
    )
    .expect("add invariant");

    // Advance the gate to its evaluating state via the agent actor path.
    app.set_state_with_actor(
        &gate.id,
        "evaluating",
        true,
        None,
        StateActorMetadata {
            actor_kind: Some("agent".to_string()),
            ..Default::default()
        },
    )
    .expect("advance gate to evaluating");

    let lease_id = create_test_lease(&app);
    crate::lease::bind_lease(&app, &gate.id, &lease_id).expect("bind lease to gate");

    execute_operation(
        &app,
        &WriteOperation::GateEvaluate(GateEvaluateOperation {
            id: gate.id.clone(),
            decision: "yes".to_string(),
            invariant: None,
            json: false,
            actor_kind: Some("agent".to_string()),
            agent_name: Some("bogus".to_string()),
            agent_model: Some("bogus".to_string()),
            agent_version: Some("bogus".to_string()),
        }),
    )
    .expect("gate evaluate should succeed");

    let view = app.show_knot(&gate.id).expect("show").expect("gate");
    for step in &view.step_history {
        if let Some(name) = step.agent_name.as_deref() {
            assert_eq!(
                name, "claude",
                "bogus --agent-name on gate evaluate must be ignored"
            );
        }
    }
}

#[test]
fn claim_with_external_lease_stamps_lease_identity_on_claim_step() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let app = open_app(&root);

    let knot = app
        .create_knot("Claim step identity", None, Some("work_item"), None)
        .expect("create");
    let lease_id = create_test_lease(&app);
    let _ = claim_with_lease(&app, &knot.id, &lease_id);

    let view = app.show_knot(&knot.id).expect("show").expect("knot");
    let first_step = view
        .step_history
        .first()
        .expect("claim should have written a step");
    assert_eq!(first_step.agent_name.as_deref(), Some("claude"));
    assert_eq!(first_step.agent_model.as_deref(), Some("opus"));
    assert_eq!(first_step.agent_version.as_deref(), Some("4.6"));
}
