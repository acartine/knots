use super::execute_operation;
use super::tests_lease_ext::{create_test_lease, open_app, setup_repo, unique_workspace};
use crate::write_queue::{NewOperation, UpdateOperation, WriteOperation};

fn new_with_lease(title: &str, lease_id: &str) -> WriteOperation {
    WriteOperation::New(NewOperation {
        title: title.to_string(),
        description: None,
        acceptance: None,
        verification_steps: vec![],
        state: None,
        profile: None,
        workflow: None,
        fast: false,
        exploration: false,
        knot_type: None,
        objective: None,
        gate_owner_kind: None,
        gate_failure_modes: vec![],
        tags: vec![],
        scope: crate::cli_scope::ScopeArgs::default(),
        lease_id: Some(lease_id.to_string()),
        json: false,
    })
}

#[test]
fn new_with_lease_flag_binds_lease_for_handoff_authorship() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let app = open_app(&root);
    let lease_id = create_test_lease(&app);

    execute_operation(&app, &new_with_lease("Lease-bound new", &lease_id))
        .expect("new should bind lease");

    let created = app
        .list_knots()
        .expect("list")
        .into_iter()
        .find(|k| k.title == "Lease-bound new")
        .expect("lease-bound knot should be created");
    assert_eq!(created.lease_id.as_deref(), Some(lease_id.as_str()));

    execute_operation(
        &app,
        &WriteOperation::Update(UpdateOperation {
            id: created.id.clone(),
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
            add_handoff_capsule: Some("handoff".to_string()),
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
            lease_id: Some(lease_id),
            json: false,
        }),
    )
    .expect("handoff should use bound lease");

    let updated = app
        .show_knot(&created.id)
        .expect("show")
        .expect("knot exists");
    let capsule = updated
        .handoff_capsules
        .last()
        .expect("handoff capsule should exist");
    assert_eq!(capsule.agentname, "claude");
    assert_ne!(capsule.agentname, "unknown");
}

#[test]
fn new_with_lease_flag_rejects_missing_non_lease_and_terminated_lease() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let app = open_app(&root);

    let missing = execute_operation(&app, &new_with_lease("Missing lease", "missing-lease"))
        .expect_err("missing lease should fail")
        .to_string();
    assert!(missing.contains("external lease was not found"));

    let work = app
        .create_knot("Not a lease", None, None, None)
        .expect("work knot");
    let non_lease = execute_operation(&app, &new_with_lease("Non lease", &work.id))
        .expect_err("non-lease knot should fail")
        .to_string();
    assert!(non_lease.contains("does not point to a lease knot"));

    let lease_id = create_test_lease(&app);
    crate::lease::terminate_lease(&app, &lease_id).expect("terminate");
    let terminated = execute_operation(&app, &new_with_lease("Terminated lease", &lease_id))
        .expect_err("terminated lease should fail")
        .to_string();
    assert!(terminated.contains("expected 'lease_ready' or 'lease_active'"));
}
