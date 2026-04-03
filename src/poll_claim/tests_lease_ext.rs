use std::path::{Path, PathBuf};

use uuid::Uuid;

use super::{claim_knot, list_queue_candidates, run_poll};
use crate::app::{App, StateActorMetadata};
use crate::cli::PollArgs;
use crate::domain::knot_type::KnotType;

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-poll-lease-ext-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace");
    root
}

fn open_app(root: &Path) -> App {
    let db_path = root.join(".knots/cache/state.sqlite");
    App::open(db_path.to_str().unwrap(), root.to_path_buf()).unwrap()
}

fn setup_repo(root: &Path) {
    use std::process::Command;
    let run = |args: &[&str]| {
        let o = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("git");
        assert!(o.status.success(), "git {:?} failed", args);
    };
    run(&["init"]);
    run(&["config", "user.email", "knots@example.com"]);
    run(&["config", "user.name", "Knots Test"]);
    std::fs::write(root.join("README.md"), "# knots\n").unwrap();
    run(&["add", "README.md"]);
    run(&["commit", "-m", "init"]);
    run(&["branch", "-M", "main"]);
}

fn agent_info() -> crate::domain::lease::AgentInfo {
    crate::domain::lease::AgentInfo {
        agent_type: "cli".to_string(),
        provider: "test".to_string(),
        agent_name: "test-agent".to_string(),
        model: "test-model".to_string(),
        model_version: "1.0".to_string(),
    }
}

fn full_actor() -> StateActorMetadata {
    StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: Some("test-agent".to_string()),
        agent_model: Some("test-model".to_string()),
        agent_version: Some("1.0".to_string()),
    }
}

fn minimal_actor() -> StateActorMetadata {
    StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: Some("test-agent".to_string()),
        ..Default::default()
    }
}

fn create_work(app: &App, title: &str) -> crate::app::KnotView {
    app.create_knot(title, None, Some("work_item"), Some("default"))
        .expect("create work knot")
}

fn create_ready_lease(app: &App, nick: &str) -> crate::app::KnotView {
    crate::lease::create_lease(
        app,
        nick,
        crate::domain::lease::LeaseType::Agent,
        Some(agent_info()),
    )
    .expect("create lease")
}

#[test]
fn lease_excluded_from_queue_candidates() {
    let root = unique_workspace();
    let app = open_app(&root);
    let lease = crate::lease::create_lease(
        &app,
        "test-lease",
        crate::domain::lease::LeaseType::Manual,
        None,
    )
    .unwrap();
    assert_eq!(lease.knot_type, KnotType::Lease);
    create_work(&app, "Work item");
    let candidates = list_queue_candidates(&app, None).unwrap();
    assert!(!candidates.iter().any(|k| k.id == lease.id));
    assert!(candidates.iter().any(|k| k.knot_type == KnotType::Work));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn claim_rejects_lease_knot() {
    let root = unique_workspace();
    let app = open_app(&root);
    let lease = crate::lease::create_lease(
        &app,
        "unclaimed",
        crate::domain::lease::LeaseType::Manual,
        None,
    )
    .unwrap();
    let err = claim_knot(&app, &lease.id, minimal_actor(), None)
        .err()
        .expect("should fail")
        .to_string();
    assert!(err.contains("is a lease and cannot be claimed"), "{err}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn claim_creates_lease_on_claim() {
    let root = unique_workspace();
    let app = open_app(&root);
    let work = create_work(&app, "Claimable work");
    let mut actor = full_actor();
    actor.agent_name = Some("claude".to_string());
    actor.agent_model = Some("opus".to_string());
    actor.agent_version = Some("4".to_string());
    claim_knot(&app, &work.id, actor, None).unwrap();
    let knot = app.show_knot(&work.id).unwrap().unwrap();
    assert!(knot.lease_id.is_some());
    let lid = knot.lease_id.as_ref().unwrap();
    let lease = app.show_knot(lid).unwrap().unwrap();
    assert_eq!(lease.knot_type, KnotType::Lease);
    assert_eq!(lease.state, "lease_active");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn claim_without_agent_name_skips_lease_creation() {
    let root = unique_workspace();
    let app = open_app(&root);
    let work = create_work(&app, "No agent name");
    let actor = StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: None,
        agent_model: None,
        agent_version: None,
    };
    claim_knot(&app, &work.id, actor, None).unwrap();
    let knot = app.show_knot(&work.id).unwrap().unwrap();
    assert!(knot.lease_id.is_none());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn run_poll_with_claim_creates_lease() {
    let root = unique_workspace();
    let app = open_app(&root);
    create_work(&app, "Poll claim work");
    let args = PollArgs {
        stage: None,
        owner: None,
        claim: true,
        json: true,
        agent_name: Some("test-agent".to_string()),
        agent_model: Some("test-model".to_string()),
        agent_version: Some("1.0".to_string()),
    };
    run_poll(&app, args).unwrap();
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn claim_with_ready_lease_activates_and_binds() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);
    let work = create_work(&app, "Ready lease test");
    let lease = create_ready_lease(&app, "ready-lease");
    assert_eq!(
        app.show_knot(&lease.id).unwrap().unwrap().state,
        "lease_ready"
    );
    let result = claim_knot(&app, &work.id, full_actor(), Some(&lease.id)).unwrap();
    assert_eq!(result.knot.lease_id.as_deref(), Some(lease.id.as_str()));
    assert_eq!(
        app.show_knot(&lease.id).unwrap().unwrap().state,
        "lease_active"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn claim_with_active_lease_rejects() {
    let root = unique_workspace();
    let app = open_app(&root);
    let work = create_work(&app, "Active lease test");
    let lease = create_ready_lease(&app, "already-active");
    let _ = crate::lease::activate_lease(&app, &lease.id);
    let err = claim_knot(&app, &work.id, minimal_actor(), Some(&lease.id))
        .err()
        .expect("should fail")
        .to_string();
    assert!(err.contains("only lease_ready is accepted"), "{err}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn claim_with_terminated_lease_rejects() {
    let root = unique_workspace();
    let app = open_app(&root);
    let work = create_work(&app, "Terminated lease test");
    let lease = create_ready_lease(&app, "terminated");
    let _ = crate::lease::activate_lease(&app, &lease.id);
    let _ = crate::lease::terminate_lease(&app, &lease.id);
    assert!(claim_knot(&app, &work.id, minimal_actor(), Some(&lease.id)).is_err());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn claim_without_external_lease_creates_one() {
    let root = unique_workspace();
    let app = open_app(&root);
    let work = create_work(&app, "No external lease");
    let result = claim_knot(&app, &work.id, full_actor(), None).unwrap();
    assert!(result.knot.lease_id.is_some());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn completion_command_includes_lease() {
    let root = unique_workspace();
    let app = open_app(&root);
    let work = create_work(&app, "Completion cmd test");
    let lease = create_ready_lease(&app, "completion-lease");
    let result = claim_knot(&app, &work.id, full_actor(), Some(&lease.id)).unwrap();
    assert!(result.completion_cmd.contains("--lease"));
    assert!(result.completion_cmd.contains(&lease.id));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn claim_with_non_lease_knot_rejects() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);
    let work = create_work(&app, "Non-lease external");
    let fake = create_work(&app, "Not a lease");
    let err = claim_knot(&app, &work.id, minimal_actor(), Some(&fake.id))
        .err()
        .expect("should fail")
        .to_string();
    assert!(err.contains("is not a lease"), "{err}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn claim_with_nonexistent_lease_rejects() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);
    let work = create_work(&app, "Nonexistent lease test");
    assert!(claim_knot(&app, &work.id, minimal_actor(), Some("no-such-id")).is_err());
    let _ = std::fs::remove_dir_all(root);
}
