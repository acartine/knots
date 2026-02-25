use super::{App, AppError, UpdateKnotPatch};
use crate::db;
use crate::domain::metadata::MetadataEntryInput;
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-app-test-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("temp workspace should be creatable");
    write_default_workflow_file(&root);
    root
}

fn write_default_workflow_file(root: &Path) {
    let workflows_path = root.join(".knots").join("workflows.toml");
    std::fs::create_dir_all(
        workflows_path
            .parent()
            .expect("workflow parent directory should exist"),
    )
    .expect("workflow parent should be creatable");
    std::fs::write(
        workflows_path,
        concat!(
            "[[workflows]]\n",
            "id = \"default\"\n",
            "description = \"default flow\"\n",
            "initial_state = \"idea\"\n",
            "states = [\n",
            "  \"idea\",\n",
            "  \"work_item\",\n",
            "  \"implementing\",\n",
            "  \"implemented\",\n",
            "  \"reviewing\",\n",
            "  \"rejected\",\n",
            "  \"refining\",\n",
            "  \"approved\",\n",
            "  \"shipped\",\n",
            "  \"deferred\",\n",
            "  \"abandoned\"\n",
            "]\n",
            "terminal_states = [\"shipped\", \"deferred\", \"abandoned\"]\n",
            "\n",
            "[[workflows.transitions]]\n",
            "from = \"idea\"\n",
            "to = \"work_item\"\n",
            "\n",
            "[[workflows.transitions]]\n",
            "from = \"work_item\"\n",
            "to = \"implementing\"\n",
            "\n",
            "[[workflows.transitions]]\n",
            "from = \"implementing\"\n",
            "to = \"implemented\"\n",
            "\n",
            "[[workflows.transitions]]\n",
            "from = \"implemented\"\n",
            "to = \"reviewing\"\n",
            "\n",
            "[[workflows.transitions]]\n",
            "from = \"reviewing\"\n",
            "to = \"approved\"\n",
            "\n",
            "[[workflows.transitions]]\n",
            "from = \"reviewing\"\n",
            "to = \"rejected\"\n",
            "\n",
            "[[workflows.transitions]]\n",
            "from = \"rejected\"\n",
            "to = \"refining\"\n",
            "\n",
            "[[workflows.transitions]]\n",
            "from = \"refining\"\n",
            "to = \"implemented\"\n",
            "\n",
            "[[workflows.transitions]]\n",
            "from = \"approved\"\n",
            "to = \"shipped\"\n",
            "\n",
            "[[workflows.transitions]]\n",
            "from = \"*\"\n",
            "to = \"deferred\"\n",
            "\n",
            "[[workflows.transitions]]\n",
            "from = \"*\"\n",
            "to = \"abandoned\"\n"
        ),
    )
    .expect("default workflow file should be writable");
}

fn write_workflow_file(root: &Path) {
    let workflows_path = root.join(".knots").join("workflows.toml");
    std::fs::create_dir_all(
        workflows_path
            .parent()
            .expect("workflow parent directory should exist"),
    )
    .expect("workflow parent should be creatable");
    std::fs::write(
        workflows_path,
        concat!(
            "[[workflows]]\n",
            "id = \"triage\"\n",
            "description = \"triage flow\"\n",
            "initial_state = \"todo\"\n",
            "states = [\"todo\", \"doing\", \"done\"]\n",
            "terminal_states = [\"done\"]\n",
            "\n",
            "[[workflows.transitions]]\n",
            "from = \"todo\"\n",
            "to = \"doing\"\n",
            "\n",
            "[[workflows.transitions]]\n",
            "from = \"doing\"\n",
            "to = \"done\"\n"
        ),
    )
    .expect("workflow file should be writable");
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

fn stripped_id(id: &str) -> &str {
    id.rsplit_once('-').map(|(_, suffix)| suffix).unwrap_or(id)
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
            Some("work_item"),
            Some("default"),
        )
        .expect("create should succeed");
    let (prefix, suffix) = created.id.rsplit_once('-').expect("id should include '-'");
    assert!(
        prefix.starts_with("knots-app-test-"),
        "id prefix should include repo slug, got '{}'",
        created.id
    );
    assert_eq!(suffix.len(), 4);
    assert!(suffix.chars().all(|ch| ch.is_ascii_hexdigit()));
    assert_eq!(created.title, "Build cache layer");
    assert_eq!(created.state, "work_item");
    assert_eq!(created.workflow_id, "default");

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
fn hierarchical_aliases_are_assigned_and_resolve_to_ids() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app =
        App::open(db_path.to_str().expect("utf8 path"), root.clone()).expect("app should open");

    let parent = app
        .create_knot("Parent", None, Some("idea"), Some("default"))
        .expect("parent knot should be created");
    let child = app
        .create_knot("Child", None, Some("idea"), Some("default"))
        .expect("child knot should be created");
    app.add_edge(&parent.id, "parent_of", &child.id)
        .expect("parent edge should be added");

    let shown_child = app
        .show_knot(&child.id)
        .expect("show by id should succeed")
        .expect("child should exist");
    let alias = shown_child.alias.expect("child should expose alias");
    assert_eq!(alias, format!("{}.1", parent.id));

    let via_alias = app
        .show_knot(&alias)
        .expect("show by alias should succeed")
        .expect("child should resolve by alias");
    assert_eq!(via_alias.id, child.id);

    let updated = app
        .set_state(&alias, "work_item", false, None)
        .expect("set_state should accept alias id");
    assert_eq!(updated.id, child.id);
    assert_eq!(updated.state, "work_item");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn stripped_ids_resolve_for_show_state_update_and_edges() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app =
        App::open(db_path.to_str().expect("utf8 path"), root.clone()).expect("app should open");

    let src = app
        .create_knot("Source", None, Some("idea"), Some("default"))
        .expect("source knot should be created");
    let dst = app
        .create_knot("Target", None, Some("idea"), Some("default"))
        .expect("target knot should be created");

    let src_short = stripped_id(&src.id).to_string();
    let dst_short = stripped_id(&dst.id).to_string();

    let shown = app
        .show_knot(&src_short)
        .expect("show should succeed")
        .expect("source knot should resolve");
    assert_eq!(shown.id, src.id);

    let set = app
        .set_state(&src_short, "work_item", false, None)
        .expect("set_state should accept stripped id");
    assert_eq!(set.id, src.id);
    assert_eq!(set.state, "work_item");

    let updated = app
        .update_knot(
            &src_short,
            UpdateKnotPatch {
                title: Some("Source updated".to_string()),
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
        )
        .expect("update_knot should accept stripped id");
    assert_eq!(updated.id, src.id);
    assert_eq!(updated.title, "Source updated");

    let added = app
        .add_edge(&src_short, "blocked_by", &dst_short)
        .expect("add_edge should accept stripped ids");
    assert_eq!(added.src, src.id);
    assert_eq!(added.dst, dst.id);

    let edges = app
        .list_edges(&src_short, "outgoing")
        .expect("list_edges should accept stripped id");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].dst, dst.id);

    let removed = app
        .remove_edge(&src_short, "blocked_by", &dst_short)
        .expect("remove_edge should accept stripped ids");
    assert_eq!(removed.src, src.id);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn stripped_id_collisions_return_ambiguous_error() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let db_path_str = db_path.to_str().expect("utf8 path").to_string();
    std::fs::create_dir_all(
        db_path
            .parent()
            .expect("db parent directory should exist for collision test"),
    )
    .expect("db parent directory should be creatable");

    let conn = db::open_connection(&db_path_str).expect("db should open");
    db::upsert_knot_warm(&conn, "alpha-t74", "Alpha").expect("alpha warm record should insert");
    db::upsert_knot_warm(&conn, "beta-t74", "Beta").expect("beta warm record should insert");
    drop(conn);

    let app = App::open(&db_path_str, root.clone()).expect("app should open");
    let err = app.show_knot("t74").expect_err("show_knot should fail for ambiguous id");
    match err {
        AppError::InvalidArgument(message) => {
            assert!(message.contains("ambiguous knot id 't74'"));
            assert!(message.contains("matches: alpha-t74, beta-t74"));
        }
        other => panic!("unexpected error for ambiguous id: {other}"),
    }

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn set_state_enforces_transition_rules_unless_forced() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app =
        App::open(db_path.to_str().expect("utf8 path"), root.clone()).expect("app should open");

    let created = app
        .create_knot("Transition test", None, Some("idea"), Some("default"))
        .expect("create should succeed");

    let invalid = app.set_state(&created.id, "reviewing", false, None);
    assert!(invalid.is_err());

    let forced = app
        .set_state(&created.id, "reviewing", true, None)
        .expect("forced transition should succeed");
    assert_eq!(forced.state, "reviewing");

    assert_eq!(count_json_files(&root.join(".knots/events")), 2);
    assert_eq!(count_json_files(&root.join(".knots/index")), 2);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn create_knot_supports_custom_workflow_and_initial_default() {
    let root = unique_workspace();
    write_workflow_file(&root);
    let db_path = root.join(".knots/cache/state.sqlite");
    let app =
        App::open(db_path.to_str().expect("utf8 path"), root.clone()).expect("app should open");

    let created = app
        .create_knot("Workflow test", None, None, Some("triage"))
        .expect("knot should be created");

    assert_eq!(created.workflow_id, "triage");
    assert_eq!(created.state, "todo");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn custom_workflow_enforces_transitions_unless_forced() {
    let root = unique_workspace();
    write_workflow_file(&root);
    let db_path = root.join(".knots/cache/state.sqlite");
    let app =
        App::open(db_path.to_str().expect("utf8 path"), root.clone()).expect("app should open");

    let created = app
        .create_knot("Workflow transition", None, None, Some("triage"))
        .expect("knot should be created");

    let invalid = app.set_state(&created.id, "done", false, None);
    assert!(invalid.is_err());

    let forced = app
        .set_state(&created.id, "done", true, None)
        .expect("forced state transition should succeed");
    assert_eq!(forced.state, "done");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn edge_commands_update_cache_and_round_trip() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app =
        App::open(db_path.to_str().expect("utf8 path"), root.clone()).expect("app should open");

    let src = app
        .create_knot("Source", None, Some("idea"), Some("default"))
        .expect("source knot should be created");
    let dst = app
        .create_knot("Target", None, Some("idea"), Some("default"))
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
        .create_knot(
            "Parity",
            Some("legacy body"),
            Some("work_item"),
            Some("default"),
        )
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
        .create_knot("Noop", None, Some("idea"), Some("default"))
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
        .create_knot("OCC", None, Some("work_item"), Some("default"))
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
            "    \"workflow_id\": \"default\",\n",
            "    \"updated_at\": \"2026-02-24T10:00:01Z\",\n",
            "    \"terminal\": false\n",
            "  }\n",
            "}\n"
        ),
    )
    .expect("index event should be writable");

    let app = App::open(&db_path_str, root.clone()).expect("app should open");
    let rehydrated = app
        .rehydrate("9")
        .expect("rehydrate should succeed")
        .expect("knot should be rehydrated");
    assert_eq!(rehydrated.id, "K-9");
    assert_eq!(
        rehydrated.description.as_deref(),
        Some("rehydrated details")
    );
    assert_eq!(rehydrated.workflow_id, "default");
    assert_eq!(rehydrated.workflow_etag.as_deref(), Some("1002"));

    let _ = std::fs::remove_dir_all(root);
}
