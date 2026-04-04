use std::path::PathBuf;

use uuid::Uuid;

use super::App;
use crate::db;
use crate::domain::lease::LeaseType;
use crate::lease;

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-deferred-sync-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    root
}

fn open_app_and_db(root: &std::path::Path) -> (App, String) {
    let db_path = root.join(".knots/cache/state.sqlite");
    let db_str = db_path.to_str().expect("utf8").to_string();
    let app = App::open(&db_str, root.to_path_buf()).expect("app should open");
    (app, db_str)
}

#[test]
fn trigger_queued_sync_noop_when_not_pending() {
    let root = unique_workspace();
    let (app, _) = open_app_and_db(&root);

    let triggered = app.trigger_queued_sync().expect("should succeed");
    assert!(!triggered, "should not trigger when sync_pending is unset");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn trigger_queued_sync_skips_when_leases_active() {
    let root = unique_workspace();
    let (app, db_str) = open_app_and_db(&root);

    let conn = db::open_connection(&db_str).expect("open");
    db::set_meta(&conn, "sync_pending", "true").expect("set meta");

    let l =
        lease::create_lease(&app, "blocker", LeaseType::Agent, None, 600).expect("create lease");
    lease::activate_lease(&app, &l.id).expect("activate");

    let triggered = app.trigger_queued_sync().expect("should succeed");
    assert!(!triggered, "should not trigger while leases still active");

    // sync_pending should still be set
    let pending = db::get_meta(&conn, "sync_pending").expect("get meta");
    assert_eq!(pending.as_deref(), Some("true"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn trigger_queued_sync_best_effort_on_failure() {
    let root = unique_workspace();
    let (app, db_str) = open_app_and_db(&root);

    let conn = db::open_connection(&db_str).expect("open");
    db::set_meta(&conn, "sync_pending", "true").expect("set meta");

    // No leases and no remote: sync will fail, but trigger returns false
    let triggered = app.trigger_queued_sync().expect("should not error");
    assert!(!triggered, "sync should fail gracefully without a remote");

    // sync_pending stays true because the actual sync failed
    let pending = db::get_meta(&conn, "sync_pending").expect("get meta");
    assert_eq!(pending.as_deref(), Some("true"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn trigger_queued_sync_not_triggered_with_remaining_leases() {
    let root = unique_workspace();
    let (app, db_str) = open_app_and_db(&root);

    let conn = db::open_connection(&db_str).expect("open");
    db::set_meta(&conn, "sync_pending", "true").expect("set meta");

    let l1 =
        lease::create_lease(&app, "first", LeaseType::Agent, None, 600).expect("create lease 1");
    let l2 =
        lease::create_lease(&app, "second", LeaseType::Agent, None, 600).expect("create lease 2");
    lease::activate_lease(&app, &l1.id).expect("activate l1");
    lease::activate_lease(&app, &l2.id).expect("activate l2");

    // Terminate only one
    lease::terminate_lease(&app, &l1.id).expect("terminate l1");

    let triggered = app.trigger_queued_sync().expect("should succeed");
    assert!(
        !triggered,
        "should not trigger while one lease remains active"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn list_knots_skips_auto_sync_when_recent_sync_succeeded() {
    let root = unique_workspace();
    let (app, db_str) = open_app_and_db(&root);

    app.create_knot("Recent sync", None, Some("work_item"), Some("default"))
        .expect("create knot");

    let conn = db::open_connection(&db_str).expect("open");
    db::set_meta(&conn, "last_sync_success_at_ms", &now_ms().to_string()).expect("set last sync");
    db::set_meta(&conn, "sync_auto_min_interval_ms", "60000").expect("set interval");

    let listed = app.list_knots().expect("list should succeed");
    assert_eq!(listed.len(), 1, "recent sync should allow cache-only reads");

    let pending = db::get_meta(&conn, "sync_pending").expect("get meta");
    assert_eq!(pending, None, "cooldown skip should not mark sync pending");

    let _ = std::fs::remove_dir_all(root);
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
