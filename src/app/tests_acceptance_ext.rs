use super::{App, CreateKnotOptions, UpdateKnotPatch};
use crate::db;
use serde_json::Value;
use uuid::Uuid;

fn unique_workspace() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("knots-app-acceptance-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("temp workspace should be creatable");
    root
}

#[test]
fn create_and_update_round_trip_acceptance() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app =
        App::open(db_path.to_str().expect("utf8 path"), root.clone()).expect("app should open");

    let created = app
        .create_knot_with_options(
            "Acceptance work",
            Some("Description"),
            Some("ready_for_implementation"),
            Some("autopilot"),
            CreateKnotOptions {
                acceptance: Some("Must survive round-trip".to_string()),
                ..CreateKnotOptions::default()
            },
        )
        .expect("create should succeed");
    assert_eq!(
        created.acceptance.as_deref(),
        Some("Must survive round-trip")
    );

    let updated = app
        .update_knot(
            &created.id,
            UpdateKnotPatch {
                acceptance: Some("Updated criteria".to_string()),
                ..UpdateKnotPatch::default()
            },
        )
        .expect("update should succeed");
    assert_eq!(updated.acceptance.as_deref(), Some("Updated criteria"));

    let cleared = app
        .update_knot(
            &created.id,
            UpdateKnotPatch {
                acceptance: Some(String::new()),
                ..UpdateKnotPatch::default()
            },
        )
        .expect("clear should succeed");
    assert_eq!(cleared.acceptance, None);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn rehydrate_restores_acceptance_from_events() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let db_path_str = db_path.to_str().expect("utf8 path").to_string();
    std::fs::create_dir_all(
        db_path
            .parent()
            .expect("db parent should exist for rehydrate test"),
    )
    .expect("db parent should be creatable");
    let conn = db::open_connection(&db_path_str).expect("db should open");
    db::upsert_knot_warm(&conn, "K-accept", "Warm acceptance").expect("warm upsert should succeed");
    drop(conn);

    let full_path = root
        .join(".knots")
        .join("events")
        .join("2026")
        .join("03")
        .join("22")
        .join("1001-knot.acceptance_set.json");
    std::fs::create_dir_all(
        full_path
            .parent()
            .expect("full event parent directory should exist"),
    )
    .expect("full event directory should be creatable");
    std::fs::write(
        &full_path,
        concat!(
            "{\n",
            "  \"event_id\": \"1001\",\n",
            "  \"occurred_at\": \"2026-03-22T10:00:00Z\",\n",
            "  \"knot_id\": \"K-accept\",\n",
            "  \"type\": \"knot.acceptance_set\",\n",
            "  \"data\": {\"acceptance\": \"Recovered from events\"}\n",
            "}\n"
        ),
    )
    .expect("full event should be writable");

    let idx_path = root
        .join(".knots")
        .join("index")
        .join("2026")
        .join("03")
        .join("22")
        .join("1002-idx.knot_head.json");
    std::fs::create_dir_all(
        idx_path
            .parent()
            .expect("index event parent directory should exist"),
    )
    .expect("index event directory should be creatable");
    std::fs::write(
        &idx_path,
        concat!(
            "{\n",
            "  \"event_id\": \"1002\",\n",
            "  \"occurred_at\": \"2026-03-22T10:00:01Z\",\n",
            "  \"type\": \"idx.knot_head\",\n",
            "  \"data\": {\n",
            "    \"knot_id\": \"K-accept\",\n",
            "    \"title\": \"Warm acceptance\",\n",
            "    \"state\": \"ready_for_implementation\",\n",
            "    \"profile_id\": \"autopilot\",\n",
            "    \"updated_at\": \"2026-03-22T10:00:01Z\",\n",
            "    \"terminal\": false\n",
            "  }\n",
            "}\n"
        ),
    )
    .expect("index event should be writable");

    let app = App::open(&db_path_str, root.clone()).expect("app should open");
    let rehydrated = app
        .rehydrate("accept")
        .expect("rehydrate should succeed")
        .expect("knot should be rehydrated");
    assert_eq!(
        rehydrated.acceptance.as_deref(),
        Some("Recovered from events")
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn json_shape_acceptance_null_when_unset() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app =
        App::open(db_path.to_str().expect("utf8 path"), root.clone()).expect("app should open");

    let created = app
        .create_knot(
            "No acceptance",
            Some("desc"),
            Some("work_item"),
            Some("default"),
        )
        .expect("create should succeed");

    let view = app
        .show_knot(&created.id)
        .expect("show should succeed")
        .expect("knot should exist");
    let json: Value = serde_json::to_value(&view).expect("serialize should succeed");

    assert!(
        json.get("acceptance").is_some(),
        "acceptance key must be present in JSON output"
    );
    assert!(
        json["acceptance"].is_null(),
        "acceptance must serialize as null when unset, got: {}",
        json["acceptance"]
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn json_shape_acceptance_string_when_set() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app =
        App::open(db_path.to_str().expect("utf8 path"), root.clone()).expect("app should open");

    let created = app
        .create_knot_with_options(
            "With acceptance",
            Some("desc"),
            Some("ready_for_implementation"),
            Some("autopilot"),
            CreateKnotOptions {
                acceptance: Some("Tests pass and coverage met".to_string()),
                ..CreateKnotOptions::default()
            },
        )
        .expect("create should succeed");

    let view = app
        .show_knot(&created.id)
        .expect("show should succeed")
        .expect("knot should exist");
    let json: Value = serde_json::to_value(&view).expect("serialize should succeed");

    assert_eq!(
        json["acceptance"].as_str(),
        Some("Tests pass and coverage met"),
        "acceptance must serialize as a string when set"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn json_shape_acceptance_null_after_clear() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app =
        App::open(db_path.to_str().expect("utf8 path"), root.clone()).expect("app should open");

    let created = app
        .create_knot_with_options(
            "Clear test",
            Some("desc"),
            Some("ready_for_implementation"),
            Some("autopilot"),
            CreateKnotOptions {
                acceptance: Some("Initial criteria".to_string()),
                ..CreateKnotOptions::default()
            },
        )
        .expect("create should succeed");

    let cleared = app
        .update_knot(
            &created.id,
            UpdateKnotPatch {
                acceptance: Some(String::new()),
                ..UpdateKnotPatch::default()
            },
        )
        .expect("clear should succeed");

    let view = app
        .show_knot(&cleared.id)
        .expect("show should succeed")
        .expect("knot should exist");
    let json: Value = serde_json::to_value(&view).expect("serialize should succeed");

    assert!(
        json.get("acceptance").is_some(),
        "acceptance key must remain present after clearing"
    );
    assert!(
        json["acceptance"].is_null(),
        "acceptance must be null after clearing, got: {}",
        json["acceptance"]
    );

    let _ = std::fs::remove_dir_all(root);
}
