use std::error::Error;
use std::path::{Path, PathBuf};

use serde_json::Value;
use uuid::Uuid;

use super::{App, AppError, UpdateKnotPatch};
use crate::db::{self, EdgeDirection};
use crate::doctor::DoctorError;
use crate::domain::state::{InvalidStateTransition, KnotState};
use crate::fsck::FsckError;
use crate::imports::ImportError;
use crate::locks::LockError;
use crate::perf::PerfError;
use crate::remote_init::RemoteInitError;
use crate::snapshots::SnapshotError;
use crate::sync::SyncError;
use crate::workflow::WorkflowError;

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
            priority: None,
            status: None,
            knot_type: None,
            add_tags: vec![],
            remove_tags: vec![],
            add_note: None,
            add_handoff_capsule: None,
            expected_workflow_etag: None,
            force: false,
        },
    );
    assert!(matches!(empty_title, Err(AppError::InvalidArgument(_))));

    let bad_priority = app.update_knot(
        &knot.id,
        UpdateKnotPatch {
            title: None,
            description: None,
            priority: Some(9),
            status: None,
            knot_type: None,
            add_tags: vec![],
            remove_tags: vec![],
            add_note: None,
            add_handoff_capsule: None,
            expected_workflow_etag: None,
            force: false,
        },
    );
    assert!(matches!(bad_priority, Err(AppError::InvalidArgument(_))));

    let no_effect_tags = app
        .update_knot(
            &knot.id,
            UpdateKnotPatch {
                title: None,
                description: None,
                priority: None,
                status: None,
                knot_type: None,
                add_tags: vec!["   ".to_string()],
                remove_tags: vec!["   ".to_string()],
                add_note: None,
                add_handoff_capsule: None,
                expected_workflow_etag: None,
                force: false,
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
                priority: None,
                status: None,
                knot_type: None,
                add_tags: vec!["alpha".to_string()],
                remove_tags: vec![],
                add_note: None,
                add_handoff_capsule: None,
                expected_workflow_etag: None,
                force: false,
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
                priority: None,
                status: None,
                knot_type: None,
                add_tags: vec![],
                remove_tags: vec!["alpha".to_string()],
                add_note: None,
                add_handoff_capsule: None,
                expected_workflow_etag: None,
                force: false,
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
        .workflow_etag
        .clone()
        .expect("created knot should have workflow etag");

    let updated = app
        .set_state(&created.id, "work_item", false, Some(&etag))
        .expect("state update should succeed");
    assert_eq!(updated.state, "work_item");

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
        AppError::Import(ImportError::InvalidRecord("bad".to_string())),
        AppError::Sync(SyncError::GitUnavailable),
        AppError::Lock(LockError::Busy(PathBuf::from("/tmp/lock"))),
        AppError::RemoteInit(RemoteInitError::NotGitRepository),
        AppError::Fsck(FsckError::Io(std::io::Error::other("fsck"))),
        AppError::Doctor(DoctorError::Io(std::io::Error::other("doctor"))),
        AppError::Snapshot(SnapshotError::Io(std::io::Error::other("snapshot"))),
        AppError::Perf(PerfError::Other("perf".to_string())),
        AppError::Workflow(WorkflowError::MissingWorkflowReference),
        AppError::ParseState(
            "bad-state"
                .parse::<KnotState>()
                .expect_err("invalid state should fail"),
        ),
        AppError::InvalidTransition(InvalidStateTransition {
            from: KnotState::Idea,
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
