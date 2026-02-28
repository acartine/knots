use std::error::Error;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde_json::json;

use super::{
    apply_rehydrate_event, ensure_profile_etag, metadata_entry_from_input, non_empty,
    normalize_tag, parse_edge_direction, rehydrate_from_events, App, AppError, RehydrateProjection,
};
use crate::db::{EdgeDirection, KnotCacheRecord};
use crate::doctor::DoctorError;
use crate::domain::knot_type::KnotType;
use crate::domain::metadata::MetadataEntryInput;
use crate::domain::state::{InvalidStateTransition, KnotState};
use crate::events::{EventWriteError, FullEvent, FullEventKind, IndexEvent, IndexEventKind};
use crate::fsck::FsckError;
use crate::locks::LockError;
use crate::perf::PerfError;
use crate::remote_init::RemoteInitError;
use crate::snapshots::SnapshotError;
use crate::sync::SyncError;
use crate::workflow::WorkflowError;

fn sample_record() -> KnotCacheRecord {
    KnotCacheRecord {
        id: "K-1".to_string(),
        title: "Title".to_string(),
        state: "idea".to_string(),
        updated_at: "2026-02-25T10:00:00Z".to_string(),
        body: None,
        description: None,
        priority: None,
        knot_type: None,
        tags: Vec::new(),
        notes: Vec::new(),
        handoff_capsules: Vec::new(),
        profile_id: "default".to_string(),
        profile_etag: Some("etag-1".to_string()),
        deferred_from_state: None,
        created_at: None,
    }
}

#[test]
fn helper_validations_cover_success_and_error_paths() {
    assert!(matches!(
        parse_edge_direction("incoming").expect("incoming should parse"),
        EdgeDirection::Incoming
    ));
    assert!(matches!(
        parse_edge_direction("out").expect("out should parse"),
        EdgeDirection::Outgoing
    ));
    assert!(matches!(
        parse_edge_direction("all").expect("all should parse"),
        EdgeDirection::Both
    ));
    assert!(parse_edge_direction("sideways").is_err());

    assert_eq!(non_empty("  x  ").as_deref(), Some("x"));
    assert!(non_empty("   ").is_none());
    assert_eq!(normalize_tag("  A-B  "), "a-b");

    let valid = metadata_entry_from_input(
        MetadataEntryInput {
            content: "note".to_string(),
            username: Some("u".to_string()),
            datetime: Some("2026-02-25T10:00:00Z".to_string()),
            agentname: Some("a".to_string()),
            model: Some("m".to_string()),
            version: Some("v".to_string()),
        },
        "2026-02-25T10:00:00Z",
    )
    .expect("valid metadata should parse");
    assert_eq!(valid.content, "note");

    let empty_content = metadata_entry_from_input(
        MetadataEntryInput {
            content: "  ".to_string(),
            username: None,
            datetime: None,
            agentname: None,
            model: None,
            version: None,
        },
        "2026-02-25T10:00:00Z",
    );
    assert!(empty_content.is_err());

    let bad_datetime = metadata_entry_from_input(
        MetadataEntryInput {
            content: "note".to_string(),
            username: None,
            datetime: Some("not-rfc3339".to_string()),
            agentname: None,
            model: None,
            version: None,
        },
        "2026-02-25T10:00:00Z",
    );
    assert!(bad_datetime.is_err());

    let current = sample_record();
    assert!(ensure_profile_etag(&current, None).is_ok());
    assert!(ensure_profile_etag(&current, Some("etag-1")).is_ok());
    assert!(matches!(
        ensure_profile_etag(&current, Some("different")),
        Err(AppError::StaleWorkflowHead { .. })
    ));
}

#[test]
fn apply_rehydrate_event_covers_known_event_types() {
    let mut projection = RehydrateProjection {
        title: "seed".to_string(),
        state: "idea".to_string(),
        updated_at: "2026-02-25T10:00:00Z".to_string(),
        body: None,
        description: None,
        priority: None,
        knot_type: KnotType::default(),
        tags: Vec::new(),
        notes: Vec::new(),
        handoff_capsules: Vec::new(),
        profile_id: String::new(),
        profile_etag: None,
        deferred_from_state: None,
        created_at: None,
    };

    let non_object = FullEvent::with_identity(
        "e0",
        "2026-02-25T10:00:00Z",
        "K-1",
        FullEventKind::KnotCreated.as_str(),
        json!("not-object"),
    );
    apply_rehydrate_event(&mut projection, &non_object);

    let created = FullEvent::with_identity(
        "e1",
        "2026-02-25T10:01:00Z",
        "K-1",
        FullEventKind::KnotCreated.as_str(),
        json!({"title":"Created","state":"work_item","profile_id":"default"}),
    );
    apply_rehydrate_event(&mut projection, &created);

    let title_set = FullEvent::with_identity(
        "e2",
        "2026-02-25T10:02:00Z",
        "K-1",
        FullEventKind::KnotTitleSet.as_str(),
        json!({"to":"Renamed"}),
    );
    apply_rehydrate_event(&mut projection, &title_set);

    let state_set = FullEvent::with_identity(
        "e3",
        "2026-02-25T10:03:00Z",
        "K-1",
        FullEventKind::KnotStateSet.as_str(),
        json!({"to":"implementing"}),
    );
    apply_rehydrate_event(&mut projection, &state_set);

    let description_set = FullEvent::with_identity(
        "e4",
        "2026-02-25T10:04:00Z",
        "K-1",
        FullEventKind::KnotDescriptionSet.as_str(),
        json!({"description":"details"}),
    );
    apply_rehydrate_event(&mut projection, &description_set);

    let priority_set = FullEvent::with_identity(
        "e5",
        "2026-02-25T10:05:00Z",
        "K-1",
        FullEventKind::KnotPrioritySet.as_str(),
        json!({"priority":2}),
    );
    apply_rehydrate_event(&mut projection, &priority_set);

    let type_set = FullEvent::with_identity(
        "e6",
        "2026-02-25T10:06:00Z",
        "K-1",
        FullEventKind::KnotTypeSet.as_str(),
        json!({"type":"task"}),
    );
    apply_rehydrate_event(&mut projection, &type_set);

    let tag_add = FullEvent::with_identity(
        "e7",
        "2026-02-25T10:07:00Z",
        "K-1",
        FullEventKind::KnotTagAdd.as_str(),
        json!({"tag":"Release"}),
    );
    apply_rehydrate_event(&mut projection, &tag_add);
    apply_rehydrate_event(&mut projection, &tag_add);

    let note = FullEvent::with_identity(
        "e8",
        "2026-02-25T10:08:00Z",
        "K-1",
        FullEventKind::KnotNoteAdded.as_str(),
        json!({
            "entry_id":"n1",
            "content":"note",
            "username":"u",
            "datetime":"2026-02-25T10:08:00Z",
            "agentname":"a",
            "model":"m",
            "version":"v"
        }),
    );
    apply_rehydrate_event(&mut projection, &note);

    let handoff = FullEvent::with_identity(
        "e9",
        "2026-02-25T10:09:00Z",
        "K-1",
        FullEventKind::KnotHandoffCapsuleAdded.as_str(),
        json!({
            "entry_id":"h1",
            "content":"handoff",
            "username":"u",
            "datetime":"2026-02-25T10:09:00Z",
            "agentname":"a",
            "model":"m",
            "version":"v"
        }),
    );
    apply_rehydrate_event(&mut projection, &handoff);

    let tag_remove = FullEvent::with_identity(
        "e10",
        "2026-02-25T10:10:00Z",
        "K-1",
        FullEventKind::KnotTagRemove.as_str(),
        json!({"tag":"release"}),
    );
    apply_rehydrate_event(&mut projection, &tag_remove);

    assert_eq!(projection.title, "Renamed");
    assert_eq!(projection.state, "implementing");
    assert_eq!(projection.profile_id, "default");
    assert_eq!(projection.priority, Some(2));
    assert_eq!(projection.knot_type, KnotType::Work);
    assert_eq!(projection.notes.len(), 1);
    assert_eq!(projection.handoff_capsules.len(), 1);
    assert!(projection.tags.is_empty());
}

#[test]
fn rehydrate_from_events_reports_missing_workflow_and_invalid_json() {
    let root = std::env::temp_dir().join(format!("knots-rehydrate-ext-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("root should be creatable");

    let missing = rehydrate_from_events(
        &root,
        "K-1",
        "Title".to_string(),
        "work_item".to_string(),
        "2026-02-25T10:00:00Z".to_string(),
    );
    assert!(matches!(missing, Err(AppError::InvalidArgument(_))));

    let full_path = root
        .join(".knots")
        .join("events")
        .join("2026")
        .join("02")
        .join("25")
        .join("bad-knot.created.json");
    std::fs::create_dir_all(
        full_path
            .parent()
            .expect("full event parent directory should exist"),
    )
    .expect("full event parent should be creatable");
    std::fs::write(&full_path, "{").expect("full event should be writable");

    let bad_full = rehydrate_from_events(
        &root,
        "K-1",
        "Title".to_string(),
        "work_item".to_string(),
        "2026-02-25T10:00:00Z".to_string(),
    );
    assert!(matches!(bad_full, Err(AppError::InvalidArgument(_))));

    std::fs::remove_file(&full_path).expect("bad full file should be removable");
    let index_path = root
        .join(".knots")
        .join("index")
        .join("2026")
        .join("02")
        .join("25")
        .join("bad-idx.knot_head.json");
    std::fs::create_dir_all(
        index_path
            .parent()
            .expect("index event parent directory should exist"),
    )
    .expect("index event parent should be creatable");
    std::fs::write(&index_path, "{").expect("index event should be writable");

    let bad_index = rehydrate_from_events(
        &root,
        "K-1",
        "Title".to_string(),
        "work_item".to_string(),
        "2026-02-25T10:00:00Z".to_string(),
    );
    assert!(matches!(bad_index, Err(AppError::InvalidArgument(_))));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn app_error_display_source_and_from_conversions_cover_variants() {
    let io: AppError = std::io::Error::other("disk").into();
    assert!(io.to_string().contains("I/O error"));
    assert!(io.source().is_some());

    let db: AppError = rusqlite::Error::InvalidQuery.into();
    assert!(db.to_string().contains("database error"));
    assert!(db.source().is_some());

    let event: AppError = EventWriteError::InvalidFileComponent {
        field: "event_id",
        value: "bad value".to_string(),
    }
    .into();
    assert!(event.to_string().contains("event write error"));

    let sync: AppError = SyncError::GitUnavailable.into();
    assert!(sync.to_string().contains("sync error"));

    let lock: AppError = LockError::Busy(PathBuf::from("/tmp/lock")).into();
    assert!(lock.to_string().contains("lock error"));

    let remote: AppError = RemoteInitError::NotGitRepository.into();
    assert!(remote.to_string().contains("remote init error"));

    let fsck: AppError = FsckError::Io(std::io::Error::other("fsck")).into();
    assert!(fsck.to_string().contains("fsck error"));

    let doctor: AppError = DoctorError::Io(std::io::Error::other("doctor")).into();
    assert!(doctor.to_string().contains("doctor error"));

    let snapshot: AppError = SnapshotError::Io(std::io::Error::other("snapshot")).into();
    assert!(snapshot.to_string().contains("snapshot error"));

    let perf: AppError = PerfError::Io(std::io::Error::other("perf")).into();
    assert!(perf.to_string().contains("perf error"));

    let workflow: AppError = WorkflowError::InvalidDefinition("bad workflow".to_string()).into();
    assert!(workflow.to_string().contains("workflow error"));

    let parse_state: AppError = KnotState::from_str("invalid-state")
        .expect_err("parse should fail")
        .into();
    assert!(parse_state.to_string().contains("state parse error"));

    let invalid_transition: AppError = InvalidStateTransition {
        from: KnotState::ReadyForPlanning,
        to: KnotState::Shipped,
    }
    .into();
    assert!(invalid_transition
        .to_string()
        .contains("invalid state transition"));

    let stale = AppError::StaleWorkflowHead {
        expected: "e1".to_string(),
        current: "e2".to_string(),
    };
    assert!(stale.to_string().contains("stale profile_etag"));
    assert!(stale.source().is_none());

    let invalid_arg = AppError::InvalidArgument("bad arg".to_string());
    assert_eq!(invalid_arg.to_string(), "bad arg");
    assert!(invalid_arg.source().is_none());

    let not_found = AppError::NotFound("K-404".to_string());
    assert!(not_found.to_string().contains("not found"));
    assert!(not_found.source().is_none());

    let not_init = AppError::NotInitialized;
    assert!(not_init.to_string().contains("not initialized"));
    assert!(not_init.source().is_none());

    let _ = IndexEvent::with_identity(
        "idx-1",
        "2026-02-25T10:00:00Z",
        IndexEventKind::KnotHead.as_str(),
        json!({"knot_id":"K-1","title":"x","state":"idea","profile_id":"default"}),
    );
}

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-app-errpath-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    root
}

fn open_app(root: &Path) -> App {
    let db_path = root.join(".knots/cache/state.sqlite");
    let db_str = db_path.to_str().expect("utf8 db path").to_string();
    App::open(&db_str, root.to_path_buf()).expect("app should open")
}

#[test]
fn default_quick_profile_id_falls_back_to_skipped_planning_profile() {
    let root = unique_workspace();
    let app = open_app(&root);

    // No quick profile configured; should fall back to first profile
    // with planning_mode == Skipped (autopilot_no_planning).
    let quick = app
        .default_quick_profile_id()
        .expect("fallback quick profile should resolve");
    assert_eq!(quick, "autopilot_no_planning");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn open_returns_not_initialized_when_knots_dir_missing() {
    let root = unique_workspace();
    assert!(!root.join(".knots").exists());

    let result = App::open(".knots/cache/state.sqlite", root.clone());
    assert!(matches!(result, Err(AppError::NotInitialized)));

    // No .knots directory created as a side effect
    assert!(!root.join(".knots").exists());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn open_succeeds_when_knots_dir_exists() {
    let root = unique_workspace();
    std::fs::create_dir_all(root.join(".knots")).expect("create .knots");

    let result = App::open(".knots/cache/state.sqlite", root.clone());
    assert!(result.is_ok());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn open_with_custom_db_path_skips_init_check() {
    let root = unique_workspace();
    let db_path = root.join("custom/state.sqlite");
    let db_str = db_path.to_str().expect("utf8 path");

    let result = App::open(db_str, root.clone());
    assert!(result.is_ok());

    let _ = std::fs::remove_dir_all(root);
}
