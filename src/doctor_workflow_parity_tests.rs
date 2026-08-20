use std::path::Path;

use rusqlite::Connection;

use super::{check_workflow_id_parity_at, fix_workflow_id_parity};
use crate::db;
use crate::doctor::DoctorStatus;
use crate::project::StorePaths;

fn unique_workspace() -> knots_test_support::TestWorkspace {
    knots_test_support::workspace("knots-workflow-parity")
}

fn open_db(root: &Path) -> Connection {
    let db_path = root.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(db_path.parent().expect("db parent should exist"))
        .expect("db parent should be creatable");
    db::open_connection(db_path.to_str().expect("utf8 db path")).expect("db should open")
}

fn insert_hot(conn: &Connection, id: &str, title: &str, workflow_id: &str) {
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
    ?1, ?2, 'implementation', '2026-04-10T00:00:00Z', NULL, NULL, NULL,
    NULL, 'work', '[]', '[]',
    '[]', '[]', '[]',
    '{}', '{}', '{}', NULL,
    ?3, 'autopilot', NULL,
    NULL, NULL, NULL
)
"#,
        rusqlite::params![id, title, workflow_id],
    )
    .expect("hot insert should succeed");
}

fn write_worktree_event(
    root: &Path,
    seq: u64,
    knot_id: &str,
    occurred_at: &str,
    include_workflow_id: bool,
) {
    write_worktree_event_with_type(
        root,
        seq,
        knot_id,
        occurred_at,
        include_workflow_id,
        Some("work"),
        true,
    );
}

fn write_worktree_event_with_type(
    root: &Path,
    seq: u64,
    knot_id: &str,
    occurred_at: &str,
    include_workflow_id: bool,
    knot_type: Option<&str>,
    include_state: bool,
) {
    let dir = root
        .join(".knots")
        .join("_worktree")
        .join(".knots")
        .join("index")
        .join("2026")
        .join("03")
        .join("12");
    std::fs::create_dir_all(&dir).expect("index dir should be creatable");
    let path = dir.join(format!("{seq:04}-idx.knot_head.json"));
    let workflow_line = if include_workflow_id {
        "    \"workflow_id\": \"work_sdlc\",\n"
    } else {
        ""
    };
    let state_line = if include_state {
        "    \"state\": \"implementation\",\n"
    } else {
        ""
    };
    let type_line = knot_type
        .map(|value| format!("    \"type\": \"{value}\",\n"))
        .unwrap_or_default();
    let body = format!(
        concat!(
            "{{\n",
            "  \"event_id\": \"{seq}\",\n",
            "  \"occurred_at\": \"{occurred_at}\",\n",
            "  \"type\": \"idx.knot_head\",\n",
            "  \"data\": {{\n",
            "    \"knot_id\": \"{knot_id}\",\n",
            "    \"title\": \"t\",\n",
            "{state_line}",
            "{workflow_line}",
            "    \"profile_id\": \"autopilot\",\n",
            "    \"updated_at\": \"{occurred_at}\",\n",
            "{type_line}",
            "    \"terminal\": false\n",
            "  }}\n",
            "}}\n"
        ),
        seq = seq,
        occurred_at = occurred_at,
        knot_id = knot_id,
        state_line = state_line,
        workflow_line = workflow_line,
        type_line = type_line,
    );
    std::fs::write(&path, body).expect("event should be writable");
}

fn count_local_repair_events(root: &Path) -> usize {
    let index_root = root.join(".knots").join("index");
    if !index_root.exists() {
        return 0;
    }
    let mut count = 0;
    let mut stack = vec![index_root];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("dir should read") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with("-idx.knot_head.json"))
            {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn check_passes_when_no_stale_events_in_worktree() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let store_paths = StorePaths {
        root: root.join(".knots"),
    };
    let check = check_workflow_id_parity_at(&store_paths).expect("check should run");
    assert_eq!(check.status, DoctorStatus::Pass);
}

#[test]
fn check_passes_when_all_latest_events_have_workflow_id() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    write_worktree_event(&root, 1, "K-1", "2026-03-12T10:00:00Z", true);
    write_worktree_event(&root, 2, "K-2", "2026-03-12T10:01:00Z", true);
    let store_paths = StorePaths {
        root: root.join(".knots"),
    };
    let check = check_workflow_id_parity_at(&store_paths).expect("check should run");
    assert_eq!(check.status, DoctorStatus::Pass);
}

#[test]
fn check_warns_when_latest_event_missing_workflow_id() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    write_worktree_event(&root, 1, "K-1", "2026-03-12T10:00:00Z", false);
    write_worktree_event(&root, 2, "K-2", "2026-03-12T10:01:00Z", true);
    let store_paths = StorePaths {
        root: root.join(".knots"),
    };
    let check = check_workflow_id_parity_at(&store_paths).expect("check should run");
    assert_eq!(check.status, DoctorStatus::Warn);
    assert!(check.detail.contains("1 knot"));
    assert!(check.detail.contains("doctor --fix"));
}

#[test]
fn check_warn_data_names_stale_head_event_and_path() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    write_worktree_event_with_type(&root, 1, "K-1", "2026-03-12T10:00:00Z", false, None, true);
    let store_paths = StorePaths {
        root: root.join(".knots"),
    };

    let check = check_workflow_id_parity_at(&store_paths).expect("check should run");
    let data = check.data.expect("warn should carry stale head data");
    let head = &data["stale_heads"][0];

    assert_eq!(head["knot_id"], "K-1");
    assert_eq!(head["event_id"], "1");
    assert_eq!(
        head["path"],
        ".knots/index/2026/03/12/0001-idx.knot_head.json"
    );
    assert_eq!(head["state"], "implementation");
    assert_eq!(head["profile_id"], "autopilot");
    assert_eq!(
        head["missing_fields"],
        serde_json::json!(["workflow_id", "type"])
    );
    assert!(check.detail.contains("K-1"));
    assert!(check.detail.contains("0001-idx.knot_head.json"));
}

#[test]
fn check_only_considers_latest_event_per_knot() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    write_worktree_event(&root, 1, "K-1", "2026-03-12T10:00:00Z", false);
    write_worktree_event(&root, 2, "K-1", "2026-03-12T10:01:00Z", true);
    let store_paths = StorePaths {
        root: root.join(".knots"),
    };
    let check = check_workflow_id_parity_at(&store_paths).expect("check should run");
    assert_eq!(
        check.status,
        DoctorStatus::Pass,
        "a newer event with workflow_id should supersede an older legacy event"
    );
}

#[test]
fn fix_emits_repair_event_for_stale_knot_in_db() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let conn = open_db(&root);
    insert_hot(&conn, "K-1", "My knot", "work_sdlc");
    drop(conn);
    write_worktree_event(&root, 1, "K-1", "2026-03-12T10:00:00Z", false);

    assert_eq!(count_local_repair_events(&root), 0);
    fix_workflow_id_parity(&root);
    assert_eq!(
        count_local_repair_events(&root),
        1,
        "one repair event should be published"
    );

    let store_paths = StorePaths {
        root: root.join(".knots"),
    };
    let after = check_workflow_id_parity_at(&store_paths).expect("recheck should run");
    assert_eq!(
        after.status,
        DoctorStatus::Warn,
        "worktree still has the stale event until the next sync push"
    );
}

#[test]
fn fix_repairs_cache_absent_stale_head_with_legacy_type_inference() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let _conn = open_db(&root);
    write_worktree_event_with_type(
        &root,
        1,
        "K-missing",
        "2026-03-12T10:00:00Z",
        false,
        None,
        true,
    );

    let summary = fix_workflow_id_parity(&root);
    assert_eq!(summary.emitted, 1);
    assert_eq!(
        count_local_repair_events(&root),
        1,
        "cache-absent legacy heads should get a repair event"
    );
    let payload = read_single_repair_payload(&root);
    assert_eq!(payload["data"]["knot_id"], "K-missing");
    assert_eq!(payload["data"]["workflow_id"], "work_sdlc");
    assert_eq!(payload["data"]["type"], "work");
    assert_eq!(payload["data"]["state"], "implementation");
}

#[test]
fn fix_does_not_duplicate_pending_local_repair_before_sync() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let _conn = open_db(&root);
    write_worktree_event_with_type(
        &root,
        1,
        "K-missing",
        "2026-03-12T10:00:00Z",
        false,
        None,
        true,
    );

    let first = fix_workflow_id_parity(&root);
    let second = fix_workflow_id_parity(&root);

    assert_eq!(first.emitted, 1);
    assert_eq!(second.emitted, 0);
    assert_eq!(second.pending, 1);
    assert_eq!(
        count_local_repair_events(&root),
        1,
        "the second run should reuse the pending local repair event"
    );
}

#[test]
fn fix_reports_precise_skip_when_stale_head_cannot_build_payload() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let _conn = open_db(&root);
    write_worktree_event_with_type(
        &root,
        1,
        "K-missing-state",
        "2026-03-12T10:00:00Z",
        false,
        Some("work"),
        false,
    );

    let summary = fix_workflow_id_parity(&root);

    assert_eq!(summary.emitted, 0);
    assert_eq!(summary.skipped, 1);
    assert!(summary.messages[0].contains("K-missing-state"));
    assert!(summary.messages[0].contains("event 1"));
    assert!(summary.messages[0].contains("lacks title, state, or updated_at"));
    assert_eq!(count_local_repair_events(&root), 0);
}

#[test]
fn fix_skips_knot_with_empty_workflow_id_in_db() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let conn = open_db(&root);
    insert_hot(&conn, "K-1", "My knot", "");
    drop(conn);
    write_worktree_event(&root, 1, "K-1", "2026-03-12T10:00:00Z", false);

    fix_workflow_id_parity(&root);
    assert_eq!(
        count_local_repair_events(&root),
        0,
        "no repair event when DB has empty workflow_id"
    );
}

#[test]
fn fix_emits_repair_event_for_cold_knot_via_cold_catalog() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let conn = open_db(&root);
    db::upsert_cold_catalog(
        &conn,
        "K-cold",
        "Cold title",
        "shipped",
        "2026-03-12T10:00:00Z",
    )
    .expect("cold insert should succeed");
    drop(conn);
    write_worktree_event(&root, 1, "K-cold", "2026-03-12T10:00:00Z", false);

    fix_workflow_id_parity(&root);
    assert_eq!(
        count_local_repair_events(&root),
        1,
        "cold knot should get a repair event from cold_catalog"
    );

    let payload = read_single_repair_payload(&root);
    assert_eq!(payload["data"]["knot_id"], "K-cold");
    assert_eq!(payload["data"]["workflow_id"], "work_sdlc");
    assert_eq!(payload["data"]["profile_id"], "autopilot");
    assert_eq!(payload["data"]["terminal"], true);
}

fn read_single_repair_payload(root: &Path) -> serde_json::Value {
    let mut stack = vec![root.join(".knots").join("index")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("dir should read") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with("-idx.knot_head.json"))
            {
                let bytes = std::fs::read(&path).expect("file");
                return serde_json::from_slice(&bytes).expect("json");
            }
        }
    }
    panic!("no repair event found");
}

#[test]
fn inferred_terminal_matches_terminal_states() {
    assert!(super::inferred_terminal("shipped"));
    assert!(super::inferred_terminal("SHIPPED"));
    assert!(!super::inferred_terminal("implementation"));
}
