use super::{rehydrate_from_events, AppError};
use crate::app::App;
use crate::db::{self, UpsertKnotHot};

fn unique_root(prefix: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("root should be creatable");
    root
}

fn write_event(root: &std::path::Path, filename: &str, body: &str) {
    let path = root
        .join(".knots")
        .join("events")
        .join("2026")
        .join("02")
        .join("25")
        .join(filename);
    std::fs::create_dir_all(path.parent().expect("event parent should exist"))
        .expect("event parent should be creatable");
    std::fs::write(path, body).expect("event should be writable");
}

fn open_app(root: &std::path::Path) -> App {
    let db_path = root.join(".knots").join("cache").join("state.sqlite");
    std::fs::create_dir_all(db_path.parent().expect("db parent should exist"))
        .expect("db parent should be creatable");
    App::open(db_path.to_str().expect("utf8 db path"), root.to_path_buf()).expect("app should open")
}

#[test]
fn rehydrate_from_events_rejects_missing_workflow_id() {
    let missing_root = unique_root("knots-rehydrate-missing-workflow");
    let missing = rehydrate_from_events(
        &missing_root,
        "K-missing",
        "Title".to_string(),
        "work_item".to_string(),
        "2026-02-25T10:00:00Z".to_string(),
    )
    .expect_err("missing workflow id should fail");
    assert!(matches!(missing, AppError::InvalidArgument(message) if
        message.contains("missing workflow_id")));

    let _ = std::fs::remove_dir_all(missing_root);
}

#[test]
fn rehydrate_from_events_converts_legacy_workflow_id() {
    let legacy_root = unique_root("knots-rehydrate-legacy-workflow");
    write_event(
        &legacy_root,
        "1000-knot.created.json",
        concat!(
            "{\n",
            "  \"event_id\": \"1000\",\n",
            "  \"type\": \"knot.created\",\n",
            "  \"occurred_at\": \"2026-02-25T10:00:00Z\",\n",
            "  \"knot_id\": \"K-legacy\",\n",
            "  \"data\": {\n",
            "    \"title\": \"Legacy\",\n",
            "    \"state\": \"ready_for_planning\",\n",
            "    \"workflow_id\": \"knots_sdlc\",\n",
            "    \"profile_id\": \"autopilot\"\n",
            "  }\n",
            "}\n"
        ),
    );
    let projection = rehydrate_from_events(
        &legacy_root,
        "K-legacy",
        "Legacy".to_string(),
        "ready_for_planning".to_string(),
        "2026-02-25T10:00:00Z".to_string(),
    )
    .expect("legacy workflow id should be converted, not rejected");
    assert_eq!(projection.workflow_id, "work_sdlc");

    let _ = std::fs::remove_dir_all(legacy_root);
}

#[test]
fn rehydrate_from_events_reports_invalid_json() {
    let root = unique_root("knots-rehydrate-invalid-json");
    write_event(&root, "bad-knot.created.json", "{");

    let bad_full = rehydrate_from_events(
        &root,
        "K-1",
        "Title".to_string(),
        "work_item".to_string(),
        "2026-02-25T10:00:00Z".to_string(),
    );
    assert!(matches!(bad_full, Err(AppError::InvalidArgument(_))));

    std::fs::remove_file(
        root.join(".knots")
            .join("events")
            .join("2026")
            .join("02")
            .join("25")
            .join("bad-knot.created.json"),
    )
    .expect("bad full file should be removable");

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
            .expect("index event parent should exist"),
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
fn show_knot_fails_when_cache_contains_legacy_workflow_id() {
    let root = unique_root("knots-show-legacy-workflow");
    let app = open_app(&root);
    db::upsert_knot_hot(
        &app.conn,
        &UpsertKnotHot {
            id: "K-legacy-db",
            title: "Legacy DB",
            state: "ready_for_planning",
            updated_at: "2026-02-25T10:00:00Z",
            body: None,
            description: None,
            acceptance: None,
            priority: None,
            knot_type: Some("work"),
            tags: &[],
            notes: &[],
            handoff_capsules: &[],
            invariants: &[],
            step_history: &[],
            gate_data: &crate::domain::gate::GateData::default(),
            lease_data: &crate::domain::lease::LeaseData::default(),
            lease_id: None,
            workflow_id: "knots_sdlc",
            profile_id: "autopilot",
            profile_etag: Some("etag-1"),
            deferred_from_state: None,
            blocked_from_state: None,
            created_at: Some("2026-02-25T10:00:00Z"),
        },
    )
    .expect("legacy row should upsert");

    let view = app
        .show_knot("K-legacy-db")
        .expect("show should succeed for legacy profile")
        .expect("knot should exist");
    assert!(
        view.step_metadata.is_none(),
        "no metadata for unknown profile"
    );

    let _ = std::fs::remove_dir_all(root);
}
