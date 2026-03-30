use std::error::Error;
use std::path::{Path, PathBuf};

use serde_json::Value;
use uuid::Uuid;

use super::{App, AppError, StateActorMetadata, UpdateKnotPatch};
use crate::db::{self, EdgeDirection};
use crate::doctor::DoctorError;
use crate::domain::state::{InvalidStateTransition, KnotState};
use crate::fsck::FsckError;
use crate::locks::LockError;
use crate::perf::PerfError;
use crate::remote_init::RemoteInitError;
use crate::snapshots::SnapshotError;
use crate::sync::SyncError;
use crate::workflow::WorkflowError;

pub(super) const CUSTOM_WORKFLOW_BUNDLE: &str = r#"
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

pub(super) fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-app-coverage-ext-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    root
}

pub(super) fn open_app(root: &Path) -> (App, String) {
    let db_path = root.join(".knots/cache/state.sqlite");
    let db_path_str = db_path.to_str().expect("utf8 db path").to_string();
    let app = App::open(&db_path_str, root.to_path_buf()).expect("app should open");
    (app, db_path_str)
}

pub(super) fn read_event_payloads(root: &Path, event_type: &str) -> Vec<Value> {
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


pub(super) fn default_update_patch() -> UpdateKnotPatch {
    UpdateKnotPatch {
        title: None, description: None, acceptance: None,
        priority: None, status: None, knot_type: None,
        add_tags: vec![], remove_tags: vec![],
        add_invariants: vec![], remove_invariants: vec![],
        clear_invariants: false, gate_owner_kind: None,
        gate_failure_modes: None, clear_gate_failure_modes: false,
        add_note: None, add_handoff_capsule: None,
        expected_profile_etag: None, force: false,
        state_actor: StateActorMetadata::default(),
    }
}

#[test]
fn update_knot_rejects_blank_title_and_bad_priority() {
    let root = unique_workspace();
    let (app, _) = open_app(&root);
    let knot = app
        .create_knot("Coverage", None, Some("idea"), Some("default"))
        .expect("knot should be created");
    let empty = app.update_knot(&knot.id, UpdateKnotPatch {
        title: Some("   ".to_string()), ..default_update_patch() });
    assert!(matches!(empty, Err(AppError::InvalidArgument(_))));
    let bad = app.update_knot(&knot.id, UpdateKnotPatch {
        priority: Some(9), ..default_update_patch() });
    assert!(matches!(bad, Err(AppError::InvalidArgument(_))));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn update_knot_tag_normalization_branches() {
    let root = unique_workspace();
    let (app, _) = open_app(&root);
    let knot = app
        .create_knot("Coverage", None, Some("idea"), Some("default"))
        .expect("knot should be created");
    let no_effect = app.update_knot(&knot.id, UpdateKnotPatch {
        add_tags: vec!["   ".to_string()], remove_tags: vec!["   ".to_string()],
        ..default_update_patch() }).expect("no-op tags should return knot state");
    assert_eq!(no_effect.id, knot.id);
    let with_tag = app.update_knot(&knot.id, UpdateKnotPatch {
        add_tags: vec!["alpha".to_string()], ..default_update_patch()
    }).expect("tag add should succeed");
    assert!(with_tag.tags.contains(&"alpha".to_string()));
    let removed = app.update_knot(&knot.id, UpdateKnotPatch {
        remove_tags: vec!["alpha".to_string()], ..default_update_patch()
    }).expect("tag remove should succeed");
    assert!(!removed.tags.contains(&"alpha".to_string()));
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
