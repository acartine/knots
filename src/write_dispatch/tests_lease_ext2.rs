use super::{execute_operation, operation_from_command};
use crate::app::StateActorMetadata;
use crate::poll_claim;
use crate::write_queue::{NextOperation, UpdateOperation, WriteOperation};

use super::tests_lease_ext::{create_test_lease, open_app, parse, setup_repo, unique_workspace};

fn blank_update(id: &str) -> UpdateOperation {
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
        gate_owner_kind: None,
        gate_failure_modes: vec![],
        clear_gate_failure_modes: false,
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
    }
}

fn update_with_note(
    id: &str,
    note: &str,
    user: Option<&str>,
    agent: Option<&str>,
    model: Option<&str>,
    ver: Option<&str>,
) -> WriteOperation {
    let mut op = blank_update(id);
    op.add_note = Some(note.to_string());
    op.note_username = user.map(String::from);
    op.note_agentname = agent.map(String::from);
    op.note_model = model.map(String::from);
    op.note_version = ver.map(String::from);
    WriteOperation::Update(op)
}

fn update_with_handoff(
    id: &str,
    content: &str,
    user: Option<&str>,
    agent: Option<&str>,
    model: Option<&str>,
    ver: Option<&str>,
) -> WriteOperation {
    let mut op = blank_update(id);
    op.add_handoff_capsule = Some(content.to_string());
    op.handoff_username = user.map(String::from);
    op.handoff_agentname = agent.map(String::from);
    op.handoff_model = model.map(String::from);
    op.handoff_version = ver.map(String::from);
    WriteOperation::Update(op)
}

fn next_op(id: &str, state: &str, lease_id: Option<&str>) -> WriteOperation {
    WriteOperation::Next(NextOperation {
        id: id.to_string(),
        expected_state: Some(state.to_string()),
        json: false,
        approve_terminal_cascade: false,
        actor_kind: None,
        agent_name: None,
        agent_model: None,
        agent_version: None,
        lease_id: lease_id.map(String::from),
    })
}

fn full_actor() -> StateActorMetadata {
    StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: Some("test-agent".to_string()),
        agent_model: Some("test-model".to_string()),
        agent_version: Some("1.0".to_string()),
    }
}

fn no_lease_actor() -> StateActorMetadata {
    StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: None,
        agent_model: None,
        agent_version: None,
    }
}

#[test]
fn explicit_note_flags_override_lease_agent_info() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);
    let knot = app.create_knot("Override test", None, None, None).unwrap();
    let lid = create_test_lease(&app);
    crate::lease::bind_lease(&app, &knot.id, &lid).unwrap();
    let op = update_with_note(
        &knot.id,
        "override note",
        Some("custom-user"),
        Some("custom-agent"),
        Some("custom-model"),
        Some("9.9"),
    );
    execute_operation(&app, &op).unwrap();
    let note = app.show_knot(&knot.id).unwrap().unwrap();
    let n = note.notes.last().unwrap();
    assert_eq!(n.username, "custom-user");
    assert_eq!(n.agentname, "custom-agent");
    assert_eq!(n.model, "custom-model");
    assert_eq!(n.version, "9.9");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn note_defaults_preserved_without_lease() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);
    let knot = app.create_knot("No lease test", None, None, None).unwrap();
    let op = update_with_note(&knot.id, "plain note", None, None, None, None);
    execute_operation(&app, &op).unwrap();
    let n = app.show_knot(&knot.id).unwrap().unwrap();
    let note = n.notes.last().unwrap();
    assert_eq!(note.username, "unknown");
    assert_eq!(note.agentname, "unknown");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn handoff_capsule_auto_fills_from_lease_agent_info() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);
    let knot = app
        .create_knot("Handoff autofill", None, None, None)
        .unwrap();
    let lid = create_test_lease(&app);
    crate::lease::bind_lease(&app, &knot.id, &lid).unwrap();
    let op = update_with_handoff(&knot.id, "auto-filled handoff", None, None, None, None);
    execute_operation(&app, &op).unwrap();
    let hc = app
        .show_knot(&knot.id)
        .unwrap()
        .unwrap()
        .handoff_capsules
        .last()
        .cloned()
        .unwrap();
    assert_eq!(hc.username, "Anthropic");
    assert_eq!(hc.agentname, "claude");
    assert_eq!(hc.model, "opus");
    assert_eq!(hc.version, "4.6");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn explicit_handoff_flags_override_lease_agent_info() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);
    let knot = app
        .create_knot("Handoff override", None, None, None)
        .unwrap();
    let lid = create_test_lease(&app);
    crate::lease::bind_lease(&app, &knot.id, &lid).unwrap();
    let op = update_with_handoff(
        &knot.id,
        "override handoff",
        Some("custom-user"),
        Some("custom-agent"),
        Some("custom-model"),
        Some("9.9"),
    );
    execute_operation(&app, &op).unwrap();
    let hc = app
        .show_knot(&knot.id)
        .unwrap()
        .unwrap()
        .handoff_capsules
        .last()
        .cloned()
        .unwrap();
    assert_eq!(hc.username, "custom-user");
    assert_eq!(hc.agentname, "custom-agent");
    assert_eq!(hc.model, "custom-model");
    assert_eq!(hc.version, "9.9");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn operation_from_lease_create_includes_json() {
    let cli = parse(&["kno", "lease", "create", "--nickname", "s", "--json"]);
    match operation_from_command(&cli.command) {
        Some(WriteOperation::LeaseCreate(c)) => assert!(c.json),
        other => panic!("expected LeaseCreate, got {:?}", other),
    }
}

#[test]
fn operation_from_new_includes_lease_id() {
    let cli = parse(&["kno", "new", "Title", "--lease", "lease-abc"]);
    match operation_from_command(&cli.command) {
        Some(WriteOperation::New(n)) => {
            assert_eq!(n.lease_id.as_deref(), Some("lease-abc"))
        }
        other => panic!("expected New, got {:?}", other),
    }
}

#[test]
fn operation_from_update_includes_lease_id() {
    let cli = parse(&["kno", "update", "k-xyz", "--lease", "lease-abc"]);
    match operation_from_command(&cli.command) {
        Some(WriteOperation::Update(u)) => {
            assert_eq!(u.lease_id.as_deref(), Some("lease-abc"))
        }
        other => panic!("expected Update, got {:?}", other),
    }
}

#[test]
fn operation_from_claim_includes_lease_id() {
    let cli = parse(&["kno", "claim", "k-xyz", "--lease", "lease-abc"]);
    match operation_from_command(&cli.command) {
        Some(WriteOperation::Claim(c)) => {
            assert_eq!(c.lease_id.as_deref(), Some("lease-abc"))
        }
        other => panic!("expected Claim, got {:?}", other),
    }
}

#[test]
fn operation_from_next_includes_lease_id() {
    let cli = parse(&[
        "kno",
        "next",
        "k-xyz",
        "--expected-state",
        "impl",
        "--lease",
        "lease-abc",
    ]);
    match operation_from_command(&cli.command) {
        Some(WriteOperation::Next(n)) => {
            assert_eq!(n.lease_id.as_deref(), Some("lease-abc"))
        }
        other => panic!("expected Next, got {:?}", other),
    }
}

#[test]
fn next_with_matching_lease_succeeds() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);
    let work = app
        .create_knot("Match lease", None, Some("work_item"), Some("default"))
        .unwrap();
    let claimed = poll_claim::claim_knot(&app, &work.id, full_actor(), None).unwrap();
    let lid = claimed.knot.lease_id.clone().unwrap();
    let op = next_op(&work.id, &claimed.knot.state, Some(&lid));
    assert!(execute_operation(&app, &op).is_ok());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn next_with_wrong_lease_fails() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);
    let work = app
        .create_knot("Wrong lease", None, Some("work_item"), Some("default"))
        .unwrap();
    let claimed = poll_claim::claim_knot(&app, &work.id, full_actor(), None).unwrap();
    let op = next_op(&work.id, &claimed.knot.state, Some("wrong-id"));
    let err = execute_operation(&app, &op).unwrap_err().to_string();
    assert!(err.contains("lease mismatch"), "{err}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn next_without_lease_works_on_unleased_knot() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);
    let work = app
        .create_knot("Unleased next", None, Some("work_item"), Some("default"))
        .unwrap();
    let claimed = poll_claim::claim_knot(&app, &work.id, no_lease_actor(), None).unwrap();
    assert!(claimed.knot.lease_id.is_none());
    let op = next_op(&work.id, &claimed.knot.state, None);
    assert!(execute_operation(&app, &op).is_ok());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn next_without_lease_on_bounded_knot_fails() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);
    let work = app
        .create_knot("Bounded", None, Some("work_item"), Some("default"))
        .unwrap();
    let claimed = poll_claim::claim_knot(&app, &work.id, full_actor(), None).unwrap();
    assert!(claimed.knot.lease_id.is_some());
    let op = next_op(&work.id, &claimed.knot.state, None);
    let err = execute_operation(&app, &op).unwrap_err().to_string();
    assert!(err.contains("active lease"), "{err}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn next_with_lease_on_unleased_knot_fails() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);
    let work = app
        .create_knot("No lease on knot", None, Some("work_item"), Some("default"))
        .unwrap();
    let claimed = poll_claim::claim_knot(&app, &work.id, no_lease_actor(), None).unwrap();
    assert!(claimed.knot.lease_id.is_none());
    let op = next_op(&work.id, &claimed.knot.state, Some("fake-lease"));
    let err = execute_operation(&app, &op).unwrap_err().to_string();
    assert!(err.contains("no active lease"), "{err}");
    let _ = std::fs::remove_dir_all(root);
}
