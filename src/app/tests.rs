use super::{App, AppError, UpdateKnotPatch};
use crate::db;
use crate::domain::metadata::MetadataEntryInput;
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-app-test-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("temp workspace should be creatable");
    root
}

fn count_json_files(root: &Path) -> usize {
    if !root.exists() {
        return 0;
    }

    let mut count = 0usize;
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let entries = std::fs::read_dir(dir).expect("directory should be readable");
        for entry in entries {
            let path = entry.expect("entry should be readable").path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|ext| ext == "json") {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn create_knot_updates_cache_and_writes_events() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app =
        App::open(db_path.to_str().expect("utf8 path"), root.clone()).expect("app should open");

    let created = app
        .create_knot(
            "Build cache layer",
            Some("Need hot/warm support"),
            "work_item",
        )
        .expect("create should succeed");
    assert!(created.id.starts_with("K-"));
    assert_eq!(created.title, "Build cache layer");
    assert_eq!(created.state, "work_item");

    let listed = app.list_knots().expect("list should succeed");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, created.id);

    let shown = app
        .show_knot(&created.id)
        .expect("show should succeed")
        .expect("knot should exist");
    assert_eq!(shown.title, created.title);

    assert_eq!(count_json_files(&root.join(".knots/events")), 1);
    assert_eq!(count_json_files(&root.join(".knots/index")), 1);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn set_state_enforces_transition_rules_unless_forced() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app =
        App::open(db_path.to_str().expect("utf8 path"), root.clone()).expect("app should open");

    let created = app
        .create_knot("Transition test", None, "idea")
        .expect("create should succeed");

    let invalid = app.set_state(&created.id, "reviewing", false, None);
    assert!(matches!(invalid, Err(AppError::InvalidTransition(_))));

    let forced = app
        .set_state(&created.id, "reviewing", true, None)
        .expect("forced transition should succeed");
    assert_eq!(forced.state, "reviewing");

    assert_eq!(count_json_files(&root.join(".knots/events")), 2);
    assert_eq!(count_json_files(&root.join(".knots/index")), 2);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn edge_commands_update_cache_and_round_trip() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app =
        App::open(db_path.to_str().expect("utf8 path"), root.clone()).expect("app should open");

    let src = app
        .create_knot("Source", None, "idea")
        .expect("source knot should be created");
    let dst = app
        .create_knot("Target", None, "idea")
        .expect("target knot should be created");

    let added = app
        .add_edge(&src.id, "blocked_by", &dst.id)
        .expect("edge should be added");
    assert_eq!(added.src, src.id);
    assert_eq!(added.kind, "blocked_by");
    assert_eq!(added.dst, dst.id);

    let outgoing = app
        .list_edges(&src.id, "outgoing")
        .expect("outgoing edges should list");
    assert_eq!(outgoing.len(), 1);
    assert_eq!(outgoing[0].dst, dst.id);

    let incoming = app
        .list_edges(&dst.id, "incoming")
        .expect("incoming edges should list");
    assert_eq!(incoming.len(), 1);
    assert_eq!(incoming[0].src, src.id);

    let removed = app
        .remove_edge(&src.id, "blocked_by", &dst.id)
        .expect("edge should be removed");
    assert_eq!(removed.src, src.id);

    let after = app
        .list_edges(&src.id, "both")
        .expect("edges should list after removal");
    assert!(after.is_empty());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn update_knot_applies_parity_fields_and_metadata_arrays() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app =
        App::open(db_path.to_str().expect("utf8 path"), root.clone()).expect("app should open");

    let created = app
        .create_knot("Parity", Some("legacy body"), "work_item")
        .expect("knot should be created");

    let updated = app
        .update_knot(
            &created.id,
            UpdateKnotPatch {
                title: Some("Parity updated".to_string()),
                description: Some("full description".to_string()),
                priority: Some(1),
                status: Some("implementing".to_string()),
                knot_type: Some("task".to_string()),
                add_tags: vec!["migration".to_string(), "beads".to_string()],
                remove_tags: vec![],
                add_note: Some(MetadataEntryInput {
                    content: "carry context".to_string(),
                    username: Some("acartine".to_string()),
                    datetime: Some("2026-02-23T10:00:00Z".to_string()),
                    agentname: Some("codex".to_string()),
                    model: Some("gpt-5".to_string()),
                    version: Some("0.1".to_string()),
                }),
                add_handoff_capsule: Some(MetadataEntryInput {
                    content: "next owner details".to_string(),
                    username: Some("acartine".to_string()),
                    datetime: Some("2026-02-23T10:05:00Z".to_string()),
                    agentname: Some("codex".to_string()),
                    model: Some("gpt-5".to_string()),
                    version: Some("0.1".to_string()),
                }),
                expected_workflow_etag: None,
                force: false,
            },
        )
        .expect("update should succeed");

    assert_eq!(updated.title, "Parity updated");
    assert_eq!(updated.state, "implementing");
    assert_eq!(updated.description.as_deref(), Some("full description"));
    assert_eq!(updated.priority, Some(1));
    assert_eq!(updated.knot_type.as_deref(), Some("task"));
    assert_eq!(
        updated.tags,
        vec!["migration".to_string(), "beads".to_string()]
    );
    assert_eq!(updated.notes.len(), 1);
    assert_eq!(updated.notes[0].content, "carry context");
    assert_eq!(updated.handoff_capsules.len(), 1);
    assert_eq!(updated.handoff_capsules[0].content, "next owner details");

    let shown = app
        .show_knot(&created.id)
        .expect("show should succeed")
        .expect("knot should exist");
    assert_eq!(shown.description.as_deref(), Some("full description"));
    assert_eq!(shown.notes.len(), 1);
    assert_eq!(shown.handoff_capsules.len(), 1);
    assert_eq!(count_json_files(&root.join(".knots/index")), 2);
    assert!(count_json_files(&root.join(".knots/events")) >= 8);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn update_knot_requires_at_least_one_change() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app =
        App::open(db_path.to_str().expect("utf8 path"), root.clone()).expect("app should open");
    let created = app
        .create_knot("Noop", None, "idea")
        .expect("knot should be created");

    let result = app.update_knot(
        &created.id,
        UpdateKnotPatch {
            title: None,
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
    assert!(matches!(result, Err(AppError::InvalidArgument(_))));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn update_knot_rejects_stale_if_match() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app =
        App::open(db_path.to_str().expect("utf8 path"), root.clone()).expect("app should open");
    let created = app
        .create_knot("OCC", None, "work_item")
        .expect("knot should be created");
    let expected = created
        .workflow_etag
        .clone()
        .expect("created knot should expose workflow_etag");

    let updated = app
        .update_knot(
            &created.id,
            UpdateKnotPatch {
                title: Some("OCC 2".to_string()),
                description: None,
                priority: None,
                status: None,
                knot_type: None,
                add_tags: vec![],
                remove_tags: vec![],
                add_note: None,
                add_handoff_capsule: None,
                expected_workflow_etag: Some(expected.clone()),
                force: false,
            },
        )
        .expect("update with matching etag should succeed");
    assert_ne!(updated.workflow_etag, Some(expected.clone()));

    let stale = app.update_knot(
        &created.id,
        UpdateKnotPatch {
            title: Some("OCC 3".to_string()),
            description: None,
            priority: None,
            status: None,
            knot_type: None,
            add_tags: vec![],
            remove_tags: vec![],
            add_note: None,
            add_handoff_capsule: None,
            expected_workflow_etag: Some(expected),
            force: false,
        },
    );
    assert!(matches!(stale, Err(AppError::StaleWorkflowHead { .. })));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn rehydrate_builds_hot_record_from_warm_and_full_events() {
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
    db::upsert_knot_warm(&conn, "K-9", "Warm title").expect("warm upsert should succeed");
    drop(conn);

    let full_path = root
        .join(".knots")
        .join("events")
        .join("2026")
        .join("02")
        .join("24")
        .join("1001-knot.description_set.json");
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
            "  \"occurred_at\": \"2026-02-24T10:00:00Z\",\n",
            "  \"knot_id\": \"K-9\",\n",
            "  \"type\": \"knot.description_set\",\n",
            "  \"data\": {\"description\": \"rehydrated details\"}\n",
            "}\n"
        ),
    )
    .expect("full event should be writable");

    let idx_path = root
        .join(".knots")
        .join("index")
        .join("2026")
        .join("02")
        .join("24")
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
            "  \"occurred_at\": \"2026-02-24T10:00:01Z\",\n",
            "  \"type\": \"idx.knot_head\",\n",
            "  \"data\": {\n",
            "    \"knot_id\": \"K-9\",\n",
            "    \"title\": \"Warm title\",\n",
            "    \"state\": \"work_item\",\n",
            "    \"updated_at\": \"2026-02-24T10:00:01Z\",\n",
            "    \"terminal\": false\n",
            "  }\n",
            "}\n"
        ),
    )
    .expect("index event should be writable");

    let app = App::open(&db_path_str, root.clone()).expect("app should open");
    let rehydrated = app
        .rehydrate("K-9")
        .expect("rehydrate should succeed")
        .expect("knot should be rehydrated");
    assert_eq!(
        rehydrated.description.as_deref(),
        Some("rehydrated details")
    );
    assert_eq!(rehydrated.workflow_etag.as_deref(), Some("1002"));

    let _ = std::fs::remove_dir_all(root);
}
