use super::{App, AppError, StateActorMetadata, UpdateKnotPatch};
use crate::db;
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
    assert_eq!(created.state, "ready_for_implementation");
    assert_eq!(created.profile_id, "autopilot");

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
        .set_state(&alias, "planning", false, None)
        .expect("set_state should accept alias id");
    assert_eq!(updated.id, child.id);
    assert_eq!(updated.state, "planning");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn partial_hierarchical_alias_resolves_to_child() {
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

    // Full alias works: "parent-id.1"
    let full_alias = format!("{}.1", parent.id);
    let via_full = app
        .show_knot(&full_alias)
        .expect("show by full alias should succeed")
        .expect("child should resolve by full alias");
    assert_eq!(via_full.id, child.id);

    // Partial alias: "suffix.1" where suffix is the short hex suffix
    let parent_suffix = stripped_id(&parent.id);
    let partial_alias = format!("{}.1", parent_suffix);
    let via_partial = app
        .show_knot(&partial_alias)
        .expect("show by partial alias should succeed")
        .expect("child should resolve by partial alias");
    assert_eq!(via_partial.id, child.id);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn partial_alias_invalid_child_index_returns_not_found() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app =
        App::open(db_path.to_str().expect("utf8 path"), root.clone()).expect("app should open");

    let parent = app
        .create_knot("Parent", None, Some("idea"), Some("default"))
        .expect("parent knot should be created");
    let _child = app
        .create_knot("Child", None, Some("idea"), Some("default"))
        .expect("child knot should be created");
    app.add_edge(&parent.id, "parent_of", &_child.id)
        .expect("parent edge should be added");

    // "suffix.99" has no matching child → NotFound
    let parent_suffix = stripped_id(&parent.id);
    let bad_alias = format!("{}.99", parent_suffix);
    let result = app.show_knot(&bad_alias);
    assert!(result.is_err(), "non-existent partial alias should error");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn partial_alias_unknown_prefix_returns_passthrough() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app =
        App::open(db_path.to_str().expect("utf8 path"), root.clone()).expect("app should open");

    // "zzzz.1" where no knot suffix matches "zzzz" → passthrough
    let result = app.show_knot("zzzz.1");
    assert!(
        result.is_ok(),
        "unknown partial alias should not hard-error"
    );
    assert!(result.unwrap().is_none(), "unknown prefix yields no knot");

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
        .set_state(&src_short, "planning", false, None)
        .expect("set_state should accept stripped id");
    assert_eq!(set.id, src.id);
    assert_eq!(set.state, "planning");

    let updated = app
        .update_knot(
            &src_short,
            UpdateKnotPatch {
                title: Some("Source updated".to_string()),
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
                lease_id: None,
                expected_profile_etag: None,
                force: false,
                state_actor: StateActorMetadata::default(),
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
    let err = app
        .show_knot("t74")
        .expect_err("show_knot should fail for ambiguous id");
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
    assert_eq!(forced.state, "implementation_review");

    assert_eq!(count_json_files(&root.join(".knots/events")), 2);
    assert_eq!(count_json_files(&root.join(".knots/index")), 2);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn create_knot_uses_default_profile_initial_state_when_state_is_omitted() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app =
        App::open(db_path.to_str().expect("utf8 path"), root.clone()).expect("app should open");

    let created = app
        .create_knot("Workflow test", None, None, Some("default"))
        .expect("knot should be created");

    assert_eq!(created.profile_id, "autopilot");
    assert_eq!(created.state, "ready_for_planning");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn unknown_workflow_is_rejected() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app =
        App::open(db_path.to_str().expect("utf8 path"), root.clone()).expect("app should open");

    let err = app
        .create_knot("Workflow transition", None, None, Some("triage"))
        .expect_err("unknown workflow should fail");
    assert!(matches!(err, AppError::Workflow(_)));

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
