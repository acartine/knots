use super::{App, AppError};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_workspace() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX_EPOCH")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("knots-app-test-{}", nanos));
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

    let invalid = app.set_state(&created.id, "reviewing", false);
    assert!(matches!(invalid, Err(AppError::InvalidTransition(_))));

    let forced = app
        .set_state(&created.id, "reviewing", true)
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
