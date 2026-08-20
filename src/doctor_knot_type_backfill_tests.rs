use std::path::Path;

use rusqlite::Connection;

use super::{check_knot_type_backfill_at, fix_knot_type_backfill};
use crate::db;
use crate::doctor::DoctorStatus;
use crate::project::StorePaths;

fn unique_workspace() -> knots_test_support::TestWorkspace {
    knots_test_support::workspace("knots-knot-type-backfill")
}

fn open_db(root: &Path) -> Connection {
    let db_path = root.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(db_path.parent().expect("db parent should exist"))
        .expect("db parent should be creatable");
    db::open_connection(db_path.to_str().expect("utf8 db path")).expect("db should open")
}

fn insert_hot_with_type(conn: &Connection, id: &str, knot_type: Option<&str>) {
    conn.execute(
        r#"
INSERT INTO knot_hot (
    id, title, state, updated_at, body, description, acceptance,
    priority, knot_type, tags_json, notes_json,
    handoff_capsules_json, invariants_json, step_history_json,
    gate_data_json, lease_data_json, execution_plan_data_json, lease_id,
    workflow_id, profile_id, profile_etag,
    deferred_from_state, blocked_from_state, created_at
)
VALUES (
    ?1, 't', 'implementation', '2026-04-10T00:00:00Z', NULL, NULL, NULL,
    NULL, ?2, '[]', '[]',
    '[]', '[]', '[]',
    '{}', '{}', '{}', NULL,
    'work_sdlc', 'autopilot', NULL,
    NULL, NULL, NULL
)
"#,
        rusqlite::params![id, knot_type],
    )
    .expect("hot insert should succeed");
}

fn write_worktree_head_event(
    root: &Path,
    seq: u64,
    knot_id: &str,
    occurred_at: &str,
    knot_type: Option<&str>,
) {
    let dir = root
        .join(".knots")
        .join("_worktree")
        .join(".knots")
        .join("index")
        .join("2026")
        .join("04")
        .join("19");
    std::fs::create_dir_all(&dir).expect("index dir should be creatable");
    let path = dir.join(format!("{seq:04}-idx.knot_head.json"));
    let type_line = match knot_type {
        Some(t) => format!("    \"type\": \"{t}\",\n"),
        None => String::new(),
    };
    let body = format!(
        concat!(
            "{{\n",
            "  \"event_id\": \"{seq}\",\n",
            "  \"occurred_at\": \"{occurred_at}\",\n",
            "  \"type\": \"idx.knot_head\",\n",
            "  \"data\": {{\n",
            "    \"knot_id\": \"{knot_id}\",\n",
            "    \"title\": \"t\",\n",
            "    \"state\": \"implementation\",\n",
            "    \"workflow_id\": \"work_sdlc\",\n",
            "{type_line}",
            "    \"profile_id\": \"autopilot\",\n",
            "    \"updated_at\": \"{occurred_at}\",\n",
            "    \"terminal\": false\n",
            "  }}\n",
            "}}\n"
        ),
        seq = seq,
        occurred_at = occurred_at,
        knot_id = knot_id,
        type_line = type_line,
    );
    std::fs::write(&path, body).expect("event should be writable");
}

fn read_knot_type(conn: &Connection, id: &str) -> Option<String> {
    conn.query_row(
        "SELECT knot_type FROM knot_hot WHERE id = ?1",
        rusqlite::params![id],
        |row| row.get::<_, Option<String>>(0),
    )
    .expect("hot lookup should succeed")
}

#[test]
fn check_passes_when_no_empty_knot_types() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let conn = open_db(&root);
    insert_hot_with_type(&conn, "K-1", Some("execution_plan"));
    insert_hot_with_type(&conn, "K-2", Some("work"));
    drop(conn);

    let store_paths = StorePaths {
        root: root.join(".knots"),
    };
    let check = check_knot_type_backfill_at(&store_paths).expect("check should run");
    assert_eq!(check.status, DoctorStatus::Pass);
}

#[test]
fn check_warns_when_hot_rows_have_empty_knot_type() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let conn = open_db(&root);
    insert_hot_with_type(&conn, "K-empty", Some(""));
    insert_hot_with_type(&conn, "K-null", None);
    insert_hot_with_type(&conn, "K-ok", Some("work"));
    drop(conn);

    let store_paths = StorePaths {
        root: root.join(".knots"),
    };
    let check = check_knot_type_backfill_at(&store_paths).expect("check should run");
    assert_eq!(check.status, DoctorStatus::Warn);
    assert!(check.detail.contains("2 hot cache row"), "{}", check.detail);
    assert!(check.detail.contains("doctor --fix"));
}

#[test]
fn fix_backfills_knot_type_from_latest_worktree_event() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let conn = open_db(&root);
    insert_hot_with_type(&conn, "K-plan", None);
    drop(conn);
    write_worktree_head_event(
        &root,
        1,
        "K-plan",
        "2026-04-19T10:00:00Z",
        Some("execution_plan"),
    );

    fix_knot_type_backfill(&root);

    let conn = open_db(&root);
    assert_eq!(
        read_knot_type(&conn, "K-plan").as_deref(),
        Some("execution_plan")
    );
}

#[test]
fn fix_uses_newest_event_when_multiple_exist_per_knot() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let conn = open_db(&root);
    insert_hot_with_type(&conn, "K-evolved", None);
    drop(conn);
    // Older event with `work`, newer event with `execution_plan` — the
    // fix must pick the later occurred_at.
    write_worktree_head_event(&root, 1, "K-evolved", "2026-04-19T10:00:00Z", Some("work"));
    write_worktree_head_event(
        &root,
        2,
        "K-evolved",
        "2026-04-19T11:00:00Z",
        Some("execution_plan"),
    );

    fix_knot_type_backfill(&root);

    let conn = open_db(&root);
    assert_eq!(
        read_knot_type(&conn, "K-evolved").as_deref(),
        Some("execution_plan")
    );
}

#[test]
fn fix_leaves_row_untouched_when_no_event_names_the_type() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let conn = open_db(&root);
    insert_hot_with_type(&conn, "K-unknown", None);
    drop(conn);
    write_worktree_head_event(&root, 1, "K-unknown", "2026-04-19T10:00:00Z", None);

    fix_knot_type_backfill(&root);

    let conn = open_db(&root);
    assert_eq!(read_knot_type(&conn, "K-unknown"), None);
}

#[test]
fn fix_ignores_knots_without_empty_knot_type() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let conn = open_db(&root);
    insert_hot_with_type(&conn, "K-ok", Some("work"));
    drop(conn);
    // A worktree event claiming a different type must not override the
    // already-populated row.
    write_worktree_head_event(
        &root,
        1,
        "K-ok",
        "2026-04-19T10:00:00Z",
        Some("execution_plan"),
    );

    fix_knot_type_backfill(&root);

    let conn = open_db(&root);
    assert_eq!(read_knot_type(&conn, "K-ok").as_deref(), Some("work"));
}
