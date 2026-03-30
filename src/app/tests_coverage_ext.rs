use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};

use serde_json::Value;
use uuid::Uuid;

use super::{App, AppError, CreateKnotOptions, GateDecision, StateActorMetadata, UpdateKnotPatch};
use crate::db::{self, EdgeDirection};
use crate::doctor::DoctorError;
use crate::domain::gate::{GateData, GateOwnerKind};
use crate::domain::invariant::{Invariant, InvariantType};
use crate::domain::state::{InvalidStateTransition, KnotState};
use crate::fsck::FsckError;
use crate::locks::LockError;
use crate::perf::PerfError;
use crate::remote_init::RemoteInitError;
use crate::snapshots::SnapshotError;
use crate::sync::SyncError;
use crate::workflow::WorkflowError;

const CUSTOM_WORKFLOW_BUNDLE: &str = r#"
[workflow]
name = "custom_flow"
version = 1
default_profile = "autopilot"

[states.ready_for_work]
kind = "queue"

[states.work]
kind = "action"
executor = "agent"
prompt = "work"

[states.done]
kind = "terminal"

[states.blocked]
kind = "escape"

[states.deferred]
kind = "escape"

[states.abandoned]
kind = "terminal"

[steps.work_step]
queue = "ready_for_work"
action = "work"

[phases.main]
produce = "work_step"
gate = "work_step"

[profiles.autopilot]
phases = ["main"]

[prompts.work]
body = "Do work"

[prompts.work.success]
complete = "done"
"#;

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-app-coverage-ext-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    root
}

fn open_app(root: &Path) -> (App, String) {
    let db_path = root.join(".knots/cache/state.sqlite");
    let db_path_str = db_path.to_str().expect("utf8 db path").to_string();
    let app = App::open(&db_path_str, root.to_path_buf()).expect("app should open");
    (app, db_path_str)
}

fn read_event_payloads(root: &Path, event_type: &str) -> Vec<Value> {
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
            if value.get("type").and_then(Value::as_str) == Some(event_type) {
                payloads.push(value);
            }
        }
    }
    payloads
}

#[test]
fn update_knot_covers_title_priority_and_tag_normalization_branches() {
    let root = unique_workspace();
    let (app, _db_path) = open_app(&root);
    let knot = app
        .create_knot("Coverage", None, Some("idea"), Some("default"))
        .expect("knot should be created");

    let empty_title = app.update_knot(
        &knot.id,
        UpdateKnotPatch {
            title: Some("   ".to_string()),
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
            gate_failure_modes: None,
            clear_gate_failure_modes: false,
            add_note: None,
            add_handoff_capsule: None,
            expected_profile_etag: None,
            force: false,
            state_actor: StateActorMetadata::default(),
        },
    );
    assert!(matches!(empty_title, Err(AppError::InvalidArgument(_))));

    let bad_priority = app.update_knot(
        &knot.id,
        UpdateKnotPatch {
            title: None,
            description: None,
            acceptance: None,
            priority: Some(9),
            status: None,
            knot_type: None,
            add_tags: vec![],
            remove_tags: vec![],
            add_invariants: vec![],
            remove_invariants: vec![],
            clear_invariants: false,
            gate_owner_kind: None,
            gate_failure_modes: None,
            clear_gate_failure_modes: false,
            add_note: None,
            add_handoff_capsule: None,
            expected_profile_etag: None,
            force: false,
            state_actor: StateActorMetadata::default(),
        },
    );
    assert!(matches!(bad_priority, Err(AppError::InvalidArgument(_))));

    let no_effect_tags = app
        .update_knot(
            &knot.id,
            UpdateKnotPatch {
                title: None,
                description: None,
                acceptance: None,
                priority: None,
                status: None,
                knot_type: None,
                add_tags: vec!["   ".to_string()],
                remove_tags: vec!["   ".to_string()],
                add_invariants: vec![],
                remove_invariants: vec![],
                clear_invariants: false,
                gate_owner_kind: None,
                gate_failure_modes: None,
                clear_gate_failure_modes: false,
                add_note: None,
                add_handoff_capsule: None,
                expected_profile_etag: None,
                force: false,
                state_actor: StateActorMetadata::default(),
            },
        )
        .expect("no-op tags should still return knot state");
    assert_eq!(no_effect_tags.id, knot.id);

    let with_tag = app
        .update_knot(
            &knot.id,
            UpdateKnotPatch {
                title: None,
                description: None,
                acceptance: None,
                priority: None,
                status: None,
                knot_type: None,
                add_tags: vec!["alpha".to_string()],
                remove_tags: vec![],
                add_invariants: vec![],
                remove_invariants: vec![],
                clear_invariants: false,
                gate_owner_kind: None,
                gate_failure_modes: None,
                clear_gate_failure_modes: false,
                add_note: None,
                add_handoff_capsule: None,
                expected_profile_etag: None,
                force: false,
                state_actor: StateActorMetadata::default(),
            },
        )
        .expect("tag add should succeed");
    assert!(with_tag.tags.contains(&"alpha".to_string()));

    let removed_tag = app
        .update_knot(
            &knot.id,
            UpdateKnotPatch {
                title: None,
                description: None,
                acceptance: None,
                priority: None,
                status: None,
                knot_type: None,
                add_tags: vec![],
                remove_tags: vec!["alpha".to_string()],
                add_invariants: vec![],
                remove_invariants: vec![],
                clear_invariants: false,
                gate_owner_kind: None,
                gate_failure_modes: None,
                clear_gate_failure_modes: false,
                add_note: None,
                add_handoff_capsule: None,
                expected_profile_etag: None,
                force: false,
                state_actor: StateActorMetadata::default(),
            },
        )
        .expect("tag remove should succeed");
    assert!(!removed_tag.tags.contains(&"alpha".to_string()));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn add_edge_rejects_blank_arguments() {
    let root = unique_workspace();
    let (app, _) = open_app(&root);

    let err = app
        .add_edge("   ", "blocked_by", "K-2")
        .expect_err("blank src should fail");
    assert!(matches!(err, AppError::InvalidArgument(_)));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cold_search_maps_cold_catalog_fields() {
    let root = unique_workspace();
    let (app, db_path) = open_app(&root);
    let conn = db::open_connection(&db_path).expect("db should open");
    db::set_meta(&conn, "sync_policy", "never").expect("sync policy should set");
    db::upsert_cold_catalog(
        &conn,
        "K-cold",
        "Cold Knot",
        "shipped",
        "2026-02-25T10:00:00Z",
    )
    .expect("cold catalog should upsert");

    let matches = app.cold_search("Cold").expect("cold search should succeed");
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].id, "K-cold");
    assert_eq!(matches[0].title, "Cold Knot");
    assert_eq!(matches[0].state, "shipped");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn set_state_with_if_match_writes_preconditions() {
    let root = unique_workspace();
    let (app, _db_path) = open_app(&root);
    let created = app
        .create_knot("State precondition", None, Some("idea"), Some("default"))
        .expect("knot should be created");
    let etag = created
        .profile_etag
        .clone()
        .expect("created knot should have workflow etag");

    let updated = app
        .set_state(&created.id, "planning", false, Some(&etag))
        .expect("state update should succeed");
    assert_eq!(updated.state, "planning");

    let mut saw_precondition = false;
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
                saw_precondition = value.get("precondition").is_some();
            }
        }
    }
    assert!(saw_precondition);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn app_error_source_covers_wrapped_error_variants() {
    let variants = vec![
        AppError::Event(crate::events::EventWriteError::Io(std::io::Error::other(
            "event",
        ))),
        AppError::Sync(SyncError::GitUnavailable),
        AppError::Lock(LockError::Busy(PathBuf::from("/tmp/lock"))),
        AppError::RemoteInit(RemoteInitError::NotGitRepository),
        AppError::Fsck(FsckError::Io(std::io::Error::other("fsck"))),
        AppError::Doctor(DoctorError::Io(std::io::Error::other("doctor"))),
        AppError::Snapshot(SnapshotError::Io(std::io::Error::other("snapshot"))),
        AppError::Perf(PerfError::Other("perf".to_string())),
        AppError::Workflow(WorkflowError::MissingProfileReference),
        AppError::ParseState(
            "bad-state"
                .parse::<KnotState>()
                .expect_err("invalid state should fail"),
        ),
        AppError::InvalidTransition(InvalidStateTransition {
            from: KnotState::ReadyForPlanning,
            to: KnotState::Shipped,
        }),
    ];

    let with_sources = variants
        .into_iter()
        .filter(|err| err.source().is_some())
        .count();
    assert!(with_sources >= 7);

    let _ = EdgeDirection::Both;
}

#[test]
fn set_profile_switches_profile_and_state_atomically_and_supports_noop() {
    let root = unique_workspace();
    let (app, _) = open_app(&root);
    let created = app
        .create_knot("Profile switch", None, Some("idea"), Some("default"))
        .expect("knot should be created");
    let etag = created
        .profile_etag
        .clone()
        .expect("created knot should expose profile etag");

    let updated = app
        .set_profile(
            &created.id,
            "autopilot_no_planning",
            "ready_for_implementation",
            Some(&etag),
        )
        .expect("profile switch should succeed");
    assert_eq!(updated.profile_id, "autopilot_no_planning");
    assert_eq!(updated.state, "ready_for_implementation");

    let before_noop_etag = updated.profile_etag.clone();
    let no_op = app
        .set_profile(
            &created.id,
            "autopilot_no_planning",
            "ready_for_implementation",
            updated.profile_etag.as_deref(),
        )
        .expect("no-op profile switch should return current state");
    assert_eq!(no_op.profile_etag, before_noop_etag);

    let profile_set_events = read_event_payloads(&root, "knot.profile_set");
    assert_eq!(profile_set_events.len(), 1);
    let event = &profile_set_events[0];
    assert_eq!(
        event
            .get("data")
            .and_then(Value::as_object)
            .and_then(|value| value.get("to_profile_id"))
            .and_then(Value::as_str),
        Some("autopilot_no_planning")
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn set_state_with_actor_records_actor_and_deferred_provenance() {
    let root = unique_workspace();
    let (app, _) = open_app(&root);
    let created = app
        .create_knot("Actor metadata", None, Some("idea"), Some("default"))
        .expect("knot should be created");

    let planning = app
        .set_state_with_actor(
            &created.id,
            "planning",
            false,
            None,
            StateActorMetadata {
                actor_kind: Some("agent".to_string()),
                agent_name: Some("codex".to_string()),
                agent_model: Some("gpt-5".to_string()),
                agent_version: Some("1".to_string()),
            },
        )
        .expect("state update with actor metadata should succeed");
    assert_eq!(planning.state, "planning");

    let deferred = app
        .set_state_with_actor(
            &created.id,
            "deferred",
            false,
            planning.profile_etag.as_deref(),
            StateActorMetadata {
                actor_kind: Some("agent".to_string()),
                agent_name: Some("codex".to_string()),
                agent_model: Some("gpt-5".to_string()),
                agent_version: Some("1".to_string()),
            },
        )
        .expect("defer transition should succeed");
    assert_eq!(deferred.state, "deferred");
    assert_eq!(deferred.deferred_from_state.as_deref(), Some("planning"));

    let resumed = app
        .set_state(
            &created.id,
            "planning",
            false,
            deferred.profile_etag.as_deref(),
        )
        .expect("resume from deferred should succeed");
    assert_eq!(resumed.state, "planning");

    let state_events = read_event_payloads(&root, "knot.state_set");
    assert!(state_events.len() >= 2);
    let actor_event = state_events
        .iter()
        .find(|event| {
            event
                .get("data")
                .and_then(Value::as_object)
                .and_then(|value| value.get("actor_kind"))
                .and_then(Value::as_str)
                == Some("agent")
        })
        .expect("actor metadata should be written to state events");
    let actor_data = actor_event
        .get("data")
        .and_then(Value::as_object)
        .expect("state event data should be object");
    assert_eq!(
        actor_data.get("agent_name").and_then(Value::as_str),
        Some("codex")
    );
    assert_eq!(
        actor_data.get("agent_model").and_then(Value::as_str),
        Some("gpt-5")
    );

    let deferred_event = state_events
        .iter()
        .find(|event| {
            event
                .get("data")
                .and_then(Value::as_object)
                .and_then(|value| value.get("to"))
                .and_then(Value::as_str)
                == Some("deferred")
        })
        .expect("deferred state event should exist");
    assert_eq!(
        deferred_event
            .get("data")
            .and_then(Value::as_object)
            .and_then(|value| value.get("deferred_from_state"))
            .and_then(Value::as_str),
        Some("planning")
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn set_profile_covers_stale_etag_and_unknown_state_paths() {
    let root = unique_workspace();
    let (app, _) = open_app(&root);
    let created = app
        .create_knot("Profile errors", None, Some("idea"), Some("default"))
        .expect("knot should be created");

    let stale = app.set_profile(
        &created.id,
        "autopilot_no_planning",
        "ready_for_implementation",
        Some("stale-etag"),
    );
    assert!(matches!(stale, Err(AppError::StaleWorkflowHead { .. })));

    let unknown_state = app.set_profile(
        &created.id,
        "autopilot_no_planning",
        "plan_review",
        created.profile_etag.as_deref(),
    );
    assert!(matches!(unknown_state, Err(AppError::Workflow(_))));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn set_state_actor_validation_and_deferred_resume_rules() {
    let root = unique_workspace();
    let (app, _) = open_app(&root);
    let created = app
        .create_knot("Deferred rules", None, Some("idea"), Some("default"))
        .expect("knot should be created");

    let invalid_actor = app.set_state_with_actor(
        &created.id,
        "planning",
        false,
        created.profile_etag.as_deref(),
        StateActorMetadata {
            actor_kind: Some("robot".to_string()),
            agent_name: None,
            agent_model: None,
            agent_version: None,
        },
    );
    assert!(matches!(invalid_actor, Err(AppError::InvalidArgument(_))));

    let deferred = app
        .set_state(
            &created.id,
            "deferred",
            false,
            created.profile_etag.as_deref(),
        )
        .expect("defer transition should succeed");
    assert_eq!(
        deferred.deferred_from_state.as_deref(),
        Some("ready_for_planning")
    );

    let bad_resume = app.set_state(
        &created.id,
        "ready_for_implementation",
        false,
        deferred.profile_etag.as_deref(),
    );
    assert!(matches!(bad_resume, Err(AppError::InvalidArgument(_))));

    let forced_resume = app
        .set_state(
            &created.id,
            "ready_for_implementation",
            true,
            deferred.profile_etag.as_deref(),
        )
        .expect("forced resume should succeed");
    assert_eq!(forced_resume.state, "ready_for_implementation");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn update_knot_state_change_writes_actor_metadata() {
    let root = unique_workspace();
    let (app, _) = open_app(&root);
    let created = app
        .create_knot("Update actor", None, Some("idea"), Some("default"))
        .expect("knot should be created");

    let updated = app
        .update_knot(
            &created.id,
            UpdateKnotPatch {
                title: None,
                description: None,
                acceptance: None,
                priority: None,
                status: Some("planning".to_string()),
                knot_type: None,
                add_tags: vec![],
                remove_tags: vec![],
                add_invariants: vec![],
                remove_invariants: vec![],
                clear_invariants: false,
                gate_owner_kind: None,
                gate_failure_modes: None,
                clear_gate_failure_modes: false,
                add_note: None,
                add_handoff_capsule: None,
                expected_profile_etag: created.profile_etag.clone(),
                force: false,
                state_actor: StateActorMetadata {
                    actor_kind: Some("agent".to_string()),
                    agent_name: Some("codex".to_string()),
                    agent_model: Some("gpt-5".to_string()),
                    agent_version: Some("1".to_string()),
                },
            },
        )
        .expect("update state change should succeed");
    assert_eq!(updated.state, "planning");

    let state_events = read_event_payloads(&root, "knot.state_set");
    let event = state_events
        .iter()
        .find(|event| {
            event
                .get("data")
                .and_then(Value::as_object)
                .and_then(|value| value.get("agent_name"))
                .and_then(Value::as_str)
                == Some("codex")
        })
        .expect("update-generated state event should include actor metadata");
    assert_eq!(
        event
            .get("data")
            .and_then(Value::as_object)
            .and_then(|value| value.get("actor_kind"))
            .and_then(Value::as_str),
        Some("agent")
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn default_profile_resolution_covers_config_and_fallback_paths() {
    let root = unique_workspace();
    let (app, _) = open_app(&root);
    let app = app.with_home_override(Some(root.clone()));

    let fallback = app
        .default_profile_id()
        .expect("fallback default profile should resolve");
    assert_eq!(fallback, "autopilot");

    let config_path = root.join(".config/knots/config.toml");
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).expect("config parent should be creatable");
    }

    std::fs::write(&config_path, "not = [valid").expect("invalid config should write");
    let invalid = app.default_profile_id();
    assert!(matches!(invalid, Err(AppError::InvalidArgument(_))));

    std::fs::write(&config_path, "default_profile = \"unknown\"\n").expect("config should write");
    let unknown = app
        .default_profile_id()
        .expect("unknown configured profile should fall back");
    assert_eq!(unknown, "autopilot");

    std::fs::write(&config_path, "default_profile = \"semiauto\"\n").expect("config should write");
    let configured = app
        .default_profile_id()
        .expect("configured profile should resolve");
    assert_eq!(configured, "semiauto");

    app.set_default_profile_id("autopilot")
        .expect("repo default profile should persist without HOME");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn workflow_specific_defaults_and_create_knot_resolve_custom_workflows() {
    let root = unique_workspace();
    let bundle = root.join("custom-flow.toml");
    std::fs::write(&bundle, CUSTOM_WORKFLOW_BUNDLE).expect("bundle should write");
    crate::installed_workflows::install_bundle(&root, &bundle).expect("bundle should install");
    crate::installed_workflows::set_current_workflow_selection(&root, "custom_flow", None, None)
        .expect("workflow selection should succeed");

    let (app, _) = open_app(&root);
    assert_eq!(
        app.default_profile_id()
            .expect("default profile should resolve"),
        "custom_flow/autopilot"
    );
    assert_eq!(
        app.default_profile_id_for_workflow("custom_flow")
            .expect("workflow profile should resolve"),
        "custom_flow/autopilot"
    );

    let created = app
        .create_knot_in_workflow("Custom work", None, None, None, Some("custom_flow"))
        .expect("workflow-specific create should succeed");
    assert_eq!(created.workflow_id, "custom_flow");
    assert_eq!(created.profile_id, "custom_flow/autopilot");
    assert_eq!(created.state, "ready_for_work");

    let wrong_profile = app.create_knot_in_workflow(
        "Wrong profile",
        None,
        None,
        Some("default"),
        Some("custom_flow"),
    );
    assert!(matches!(wrong_profile, Err(AppError::InvalidArgument(_))));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn create_knot_with_namespaced_profile_uses_profile_workflow_without_explicit_workflow() {
    let root = unique_workspace();
    let bundle = root.join("custom-flow.toml");
    std::fs::write(&bundle, CUSTOM_WORKFLOW_BUNDLE).expect("bundle should write");
    crate::installed_workflows::install_bundle(&root, &bundle).expect("bundle should install");

    let (app, _) = open_app(&root);
    let created = app
        .create_knot_in_workflow(
            "Namespaced profile",
            None,
            None,
            Some("custom_flow/autopilot"),
            None,
        )
        .expect("create should resolve workflow from namespaced profile");
    assert_eq!(created.workflow_id, "custom_flow");
    assert_eq!(created.profile_id, "custom_flow/autopilot");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn default_profile_for_workflow_falls_back_to_first_available_profile() {
    let root = unique_workspace();
    let bundle = root.join("custom-flow.toml");
    let no_default_bundle = CUSTOM_WORKFLOW_BUNDLE
        .replace("default_profile = \"autopilot\"\n", "")
        .replace(
            "[profiles.autopilot]\nphases = [\"main\"]\n",
            "[profiles.beta]\nphases = [\"main\"]\n\n[profiles.alpha]\nphases = [\"main\"]\n",
        );
    std::fs::write(&bundle, no_default_bundle).expect("bundle should write");
    crate::installed_workflows::install_bundle(&root, &bundle).expect("bundle should install");
    crate::installed_workflows::set_current_workflow_selection(&root, "custom_flow", None, None)
        .expect("workflow selection should succeed");

    let (app, _) = open_app(&root);
    assert_eq!(
        app.default_profile_id_for_workflow("custom_flow")
            .expect("workflow profile should resolve"),
        "custom_flow/alpha"
    );
    assert_eq!(
        app.default_profile_id()
            .expect("default workflow profile should resolve"),
        "custom_flow/alpha"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn evaluate_gate_failure_reopens_linked_knots_and_adds_metadata() {
    let root = unique_workspace();
    let (app, _) = open_app(&root);
    let target = app
        .create_knot("Target work", None, Some("shipped"), Some("default"))
        .expect("target knot should be created");

    let mut failure_modes = BTreeMap::new();
    failure_modes.insert("release blocked".to_string(), vec![target.id.clone()]);
    let gate = app
        .create_knot_with_options(
            "Release gate",
            Some("Gate must pass before shipment"),
            None,
            Some("default"),
            None,
            CreateKnotOptions {
                knot_type: crate::domain::knot_type::KnotType::Gate,
                gate_data: GateData {
                    owner_kind: GateOwnerKind::Human,
                    failure_modes,
                },
                ..CreateKnotOptions::default()
            },
        )
        .expect("gate should be created");
    let gate = app
        .update_knot(
            &gate.id,
            UpdateKnotPatch {
                title: None,
                description: None,
                acceptance: None,
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
        .expect("gate invariants should update");
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
            Some("release blocked"),
            StateActorMetadata::default(),
        )
        .expect("gate evaluation should succeed");

    assert_eq!(result.decision, "no");
    assert_eq!(result.gate.state, "abandoned");
    assert_eq!(result.reopened, vec![target.id.clone()]);

    let reopened = app
        .show_knot(&target.id)
        .expect("show should succeed")
        .expect("target knot should exist");
    assert_eq!(reopened.state, "ready_for_planning");
    assert!(reopened
        .notes
        .last()
        .expect("note should be added")
        .content
        .contains("reopened this knot for planning"));
    assert!(reopened
        .handoff_capsules
        .last()
        .expect("handoff should be added")
        .content
        .contains("reopened this knot for planning"));

    let _ = std::fs::remove_dir_all(root);
}
