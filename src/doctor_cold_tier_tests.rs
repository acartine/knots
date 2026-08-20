use std::path::Path;

use rusqlite::Connection;
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

use super::{check_cold_tier_imbalance, check_cold_tier_imbalance_at, fix_cold_tier_imbalance};
use crate::db;
use crate::doctor::DoctorStatus;
use crate::project::StorePaths;

fn unique_workspace() -> knots_test_support::TestWorkspace {
    knots_test_support::workspace("knots-cold-tier")
}

fn open_db(root: &Path) -> (String, Connection) {
    let db_path = root.join(".knots/cache/state.sqlite");
    let db_path_str = db_path.to_str().expect("utf8 db path").to_string();
    std::fs::create_dir_all(db_path.parent().expect("db parent should exist"))
        .expect("db parent should be creatable");
    let conn = db::open_connection(&db_path_str).expect("db should open");
    (db_path_str, conn)
}

fn fmt_rfc3339(ts: OffsetDateTime) -> String {
    ts.format(&Rfc3339).expect("rfc3339")
}

fn now_minus_hours(hours: i64) -> String {
    fmt_rfc3339(OffsetDateTime::now_utc() - Duration::hours(hours))
}

fn insert_hot_with_state(conn: &Connection, id: &str, title: &str, state: &str, updated_at: &str) {
    let tags = "[]";
    let notes = "[]";
    let handoff = "[]";
    let invariants = "[]";
    let history = "[]";
    let gate = "{}";
    let lease = "{}";
    let plan = "{}";
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
    ?1, ?2, ?11, ?12, NULL, NULL, NULL,
    NULL, 'work', ?3, ?4,
    ?5, ?6, ?7,
    ?8, ?9, ?10, NULL,
    'work_sdlc', 'autopilot', NULL,
    NULL, NULL, NULL
)
"#,
        rusqlite::params![
            id, title, tags, notes, handoff, invariants, history, gate, lease, plan, state,
            updated_at,
        ],
    )
    .expect("hot insert should succeed");
}

fn insert_hot(conn: &Connection, id: &str, title: &str) {
    insert_hot_with_state(conn, id, title, "implementation", "2026-04-10T00:00:00Z");
}

fn insert_cold_with_state(conn: &Connection, id: &str, title: &str, state: &str, updated_at: &str) {
    db::upsert_cold_catalog(conn, id, title, state, updated_at)
        .expect("cold insert should succeed");
    db::upsert_knot_warm(conn, id, title).expect("warm insert should succeed");
}

fn insert_old_terminal_cold(conn: &Connection, id: &str) {
    insert_cold_with_state(conn, id, "old-shipped", "shipped", "2024-01-01T00:00:00Z");
}

fn write_knot_head_event(root: &Path, seq: u64, id: &str, title: &str, updated_at: &str) {
    let ym_dir = root
        .join(".knots")
        .join("index")
        .join("2026")
        .join("04")
        .join("10");
    std::fs::create_dir_all(&ym_dir).expect("index dir should be creatable");
    let path = ym_dir.join(format!("{seq:04}-idx.knot_head.json"));
    std::fs::write(
        &path,
        format!(
            concat!(
                "{{\n",
                "  \"event_id\": \"{seq}\",\n",
                "  \"occurred_at\": \"2026-04-10T00:00:00Z\",\n",
                "  \"type\": \"idx.knot_head\",\n",
                "  \"data\": {{\n",
                "    \"knot_id\": \"{id}\",\n",
                "    \"title\": \"{title}\",\n",
                "    \"state\": \"implementation\",\n",
                "    \"workflow_id\": \"work_sdlc\",\n",
                "    \"profile_id\": \"autopilot\",\n",
                "    \"updated_at\": \"{updated_at}\",\n",
                "    \"terminal\": false\n",
                "  }}\n",
                "}}\n"
            ),
            seq = seq,
            id = id,
            title = title,
            updated_at = updated_at
        ),
    )
    .expect("index event should be writable");
}

#[test]
fn check_passes_when_cold_holds_only_old_terminal_knots() {
    // The exact configuration the user reported: small hot, lots of legitimately-old
    // cold rows. Today's check warns. The new check passes — this is the regression
    // lock.
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let (_, conn) = open_db(&root);
    for i in 0..5 {
        insert_hot(&conn, &format!("H-{i:03}"), "hot");
    }
    for i in 0..50 {
        insert_old_terminal_cold(&conn, &format!("C-{i:03}"));
    }

    let check = check_cold_tier_imbalance(&conn).expect("check should run");
    assert_eq!(check.status, DoctorStatus::Pass);
    let data = check.data.as_ref().expect("data");
    assert_eq!(data["hot_count"], 5);
    assert_eq!(data["cold_count"], 50);
    assert_eq!(data["shadow"], 0);
    assert_eq!(data["non_terminal_cold"], 0);
    assert_eq!(data["stale_terminal_hot"], 0);
}

#[test]
fn check_passes_with_only_recently_terminated_hot_knots() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let (_, conn) = open_db(&root);
    insert_hot_with_state(
        &conn,
        "H-1",
        "fresh-shipped",
        "shipped",
        &now_minus_hours(10),
    );
    let check = check_cold_tier_imbalance(&conn).expect("check should run");
    assert_eq!(check.status, DoctorStatus::Pass);
}

#[test]
fn check_warns_on_shadow_rows() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let (_, conn) = open_db(&root);
    insert_hot(&conn, "DUP", "shared");
    insert_old_terminal_cold(&conn, "DUP");

    let check = check_cold_tier_imbalance(&conn).expect("check should run");
    assert_eq!(check.status, DoctorStatus::Warn);
    let data = check.data.as_ref().expect("data");
    assert_eq!(data["shadow"], 1);
    assert!(check.detail.contains("shadow=1"));
}

#[test]
fn check_warns_on_non_terminal_cold_rows() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let (_, conn) = open_db(&root);
    insert_cold_with_state(
        &conn,
        "C-bad",
        "non-terminal",
        "implementation",
        "2024-01-01T00:00:00Z",
    );

    let check = check_cold_tier_imbalance(&conn).expect("check should run");
    assert_eq!(check.status, DoctorStatus::Warn);
    let data = check.data.as_ref().expect("data");
    assert_eq!(data["non_terminal_cold"], 1);
    assert!(check.detail.contains("non_terminal_cold=1"));
}

#[test]
fn check_warns_on_stale_terminal_hot_rows() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let (_, conn) = open_db(&root);
    // 100h ago > 72h cutoff → stale terminal in hot.
    insert_hot_with_state(
        &conn,
        "H-stale",
        "stale-shipped",
        "shipped",
        &now_minus_hours(100),
    );

    let check = check_cold_tier_imbalance(&conn).expect("check should run");
    assert_eq!(check.status, DoctorStatus::Warn);
    let data = check.data.as_ref().expect("data");
    assert_eq!(data["stale_terminal_hot"], 1);
    assert!(check.detail.contains("stale_terminal_hot=1"));
}

#[test]
fn check_warns_with_combined_violations_and_reports_all_counts() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let (_, conn) = open_db(&root);
    // shadow
    insert_hot(&conn, "DUP", "shared");
    insert_old_terminal_cold(&conn, "DUP");
    // non-terminal cold
    insert_cold_with_state(
        &conn,
        "C-bad",
        "non-terminal",
        "implementation",
        "2024-01-01T00:00:00Z",
    );
    // stale-terminal hot
    insert_hot_with_state(
        &conn,
        "H-stale",
        "stale-shipped",
        "shipped",
        &now_minus_hours(100),
    );

    let check = check_cold_tier_imbalance(&conn).expect("check should run");
    assert_eq!(check.status, DoctorStatus::Warn);
    let data = check.data.as_ref().expect("data");
    assert_eq!(data["shadow"], 1);
    assert_eq!(data["non_terminal_cold"], 1);
    assert_eq!(data["stale_terminal_hot"], 1);
    assert!(check.detail.contains("shadow=1"));
    assert!(check.detail.contains("non_terminal_cold=1"));
    assert!(check.detail.contains("stale_terminal_hot=1"));
}

#[test]
fn check_at_handles_missing_db_as_pass() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let store_paths = StorePaths {
        root: root.join(".knots"),
    };
    let check = check_cold_tier_imbalance_at(&store_paths).expect("check should run");
    assert_eq!(check.status, DoctorStatus::Pass);
    assert!(check.detail.contains("no cache database found"));
}

#[test]
fn fix_noop_when_db_missing() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    fix_cold_tier_imbalance(&root);
}

#[test]
fn fix_prunes_shadow_rows_and_clears_warn() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    {
        let (_, conn) = open_db(&root);
        insert_hot(&conn, "DUP", "shared");
        insert_old_terminal_cold(&conn, "DUP");
    }

    fix_cold_tier_imbalance(&root);

    let (_, conn) = open_db(&root);
    let cold = db::count_cold_catalog(&conn).expect("count");
    assert_eq!(cold, 0, "shadow cold row should be pruned");
    let hot = db::count_knot_hot(&conn).expect("count");
    assert_eq!(hot, 1, "hot row untouched by shadow prune");
    let check = check_cold_tier_imbalance(&conn).expect("check");
    assert_eq!(check.status, DoctorStatus::Pass);
}

#[test]
fn fix_rehydrates_non_terminal_cold_rows() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    {
        let (_, conn) = open_db(&root);
        insert_cold_with_state(
            &conn,
            "C-bad",
            "non-terminal",
            "implementation",
            "2026-04-10T00:00:00Z",
        );
    }
    write_knot_head_event(&root, 1001, "C-bad", "non-terminal", "2026-04-10T00:00:00Z");

    fix_cold_tier_imbalance(&root);

    let (_, conn) = open_db(&root);
    let cold = db::count_cold_catalog(&conn).expect("count");
    assert_eq!(cold, 0, "non-terminal cold row should be rehydrated out");
    let hot = db::count_knot_hot(&conn).expect("count");
    assert_eq!(hot, 1, "knot moved into hot");
    let check = check_cold_tier_imbalance(&conn).expect("check");
    assert_eq!(check.status, DoctorStatus::Pass);
}

#[test]
fn fix_drops_cold_pointer_when_rehydrate_events_missing() {
    // Non-terminal cold row, no events on disk to rebuild from. The fix
    // must drop the cold pointer (not loop forever). Warm row stays.
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    {
        let (_, conn) = open_db(&root);
        insert_cold_with_state(
            &conn,
            "C-orphan",
            "no-events",
            "implementation",
            "2026-04-10T00:00:00Z",
        );
    }

    fix_cold_tier_imbalance(&root);

    let (_, conn) = open_db(&root);
    let cold = db::count_cold_catalog(&conn).expect("count");
    assert_eq!(cold, 0, "orphan cold row should be deleted");
    let warm = db::get_knot_warm(&conn, "C-orphan")
        .expect("query")
        .is_some();
    assert!(warm, "warm catalog entry should remain");
    let check = check_cold_tier_imbalance(&conn).expect("check");
    assert_eq!(check.status, DoctorStatus::Pass);
}

#[test]
fn fix_demotes_stale_terminal_hot_rows_to_cold() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    {
        let (_, conn) = open_db(&root);
        insert_hot_with_state(
            &conn,
            "H-stale",
            "stale-shipped",
            "shipped",
            &now_minus_hours(100),
        );
    }

    fix_cold_tier_imbalance(&root);

    let (_, conn) = open_db(&root);
    let hot = db::count_knot_hot(&conn).expect("count");
    assert_eq!(hot, 0, "stale-terminal hot row should be demoted");
    let cold = db::count_cold_catalog(&conn).expect("count");
    assert_eq!(cold, 1, "demoted row should land in cold");
    let check = check_cold_tier_imbalance(&conn).expect("check");
    assert_eq!(check.status, DoctorStatus::Pass);
}

#[test]
fn fix_is_idempotent_after_clearing_violations() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    {
        let (_, conn) = open_db(&root);
        insert_hot(&conn, "DUP", "shared");
        insert_old_terminal_cold(&conn, "DUP");
        insert_hot_with_state(
            &conn,
            "H-stale",
            "stale-shipped",
            "shipped",
            &now_minus_hours(100),
        );
    }

    fix_cold_tier_imbalance(&root);
    // Snapshot post-first-fix counts.
    let (hot1, cold1) = {
        let (_, conn) = open_db(&root);
        (
            db::count_knot_hot(&conn).expect("count"),
            db::count_cold_catalog(&conn).expect("count"),
        )
    };
    fix_cold_tier_imbalance(&root);
    let (hot2, cold2) = {
        let (_, conn) = open_db(&root);
        (
            db::count_knot_hot(&conn).expect("count"),
            db::count_cold_catalog(&conn).expect("count"),
        )
    };
    assert_eq!(hot1, hot2, "second fix should not alter hot count");
    assert_eq!(cold1, cold2, "second fix should not alter cold count");

    let (_, conn) = open_db(&root);
    let check = check_cold_tier_imbalance(&conn).expect("check");
    assert_eq!(check.status, DoctorStatus::Pass);
}

#[test]
fn fix_passes_again_after_simulated_steady_state_cycle() {
    // Captures the user's bug: after --fix, normal flows that re-archive
    // (sweep moving stale-terminal hot -> cold; sync re-applying terminal
    // events into cold) must not re-introduce the warning.
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    {
        let (_, conn) = open_db(&root);
        for i in 0..5 {
            insert_hot(&conn, &format!("H-{i:03}"), "hot");
        }
        for i in 0..10 {
            insert_old_terminal_cold(&conn, &format!("C-{i:03}"));
        }
    }
    fix_cold_tier_imbalance(&root);

    // Simulate a sweep that moves a stale-terminal hot row to cold.
    {
        let (_, conn) = open_db(&root);
        insert_hot_with_state(
            &conn,
            "H-stale",
            "stale-shipped",
            "shipped",
            &now_minus_hours(100),
        );
        // ... and then archival demotes it.
        db::upsert_cold_catalog(
            &conn,
            "H-stale",
            "stale-shipped",
            "shipped",
            &now_minus_hours(100),
        )
        .expect("upsert cold");
        crate::db::delete_knot_hot(&conn, "H-stale").expect("delete hot");
    }
    // Simulate a sync re-applying a terminal event into cold (no shadow,
    // because sync deletes from hot first — we encode that by not inserting
    // the corresponding hot row).
    {
        let (_, conn) = open_db(&root);
        db::upsert_cold_catalog(
            &conn,
            "C-from-sync",
            "synced-shipped",
            "shipped",
            &now_minus_hours(200),
        )
        .expect("upsert cold");
    }

    let (_, conn) = open_db(&root);
    let check = check_cold_tier_imbalance(&conn).expect("check");
    assert_eq!(
        check.status,
        DoctorStatus::Pass,
        "doctor should remain pass through normal sweep + sync activity, got: {}",
        check.detail
    );
}
