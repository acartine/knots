use std::path::Path;

use crate::db;

use super::SyncService;

fn unique_workspace() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "knots-sync-test-{}",
        uuid::Uuid::now_v7()
    ));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    root
}

fn run_git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_repo(root: &Path) {
    run_git(root, &["init"]);
    run_git(root, &["config", "user.email", "knots@example.com"]);
    run_git(root, &["config", "user.name", "Knots Test"]);

    std::fs::write(root.join("README.md"), "# test\n")
        .expect("readme should be writable");
    run_git(root, &["add", "README.md"]);
    run_git(root, &["commit", "-m", "init"]);
    run_git(root, &["branch", "-M", "main"]);
}

fn open_sync_db(root: &Path) -> rusqlite::Connection {
    let db_path = root.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(
        db_path
            .parent()
            .expect("db parent should exist for sync test"),
    )
    .expect("db parent should be creatable");
    let conn =
        db::open_connection(db_path.to_str().expect("utf8 path"))
            .expect("sync test database should open");
    db::set_meta(&conn, "hot_window_days", "365")
        .expect("hot_window_days should be settable");
    conn
}

fn write_stale_index_event(root: &Path) {
    let idx_path = root
        .join(".knots/index/2026/02/24/0300-idx.knot_head.json");
    std::fs::create_dir_all(
        idx_path.parent().expect("idx parent should exist"),
    )
    .expect("idx dir should be creatable");
    std::fs::write(
        &idx_path,
        concat!(
            "{\n",
            "  \"event_id\": \"0300\",\n",
            "  \"occurred_at\": \"2026-02-24T10:00:00Z\",\n",
            "  \"type\": \"idx.knot_head\",\n",
            "  \"data\": {\n",
            "    \"knot_id\": \"K-occ\",\n",
            "    \"title\": \"Original title\",\n",
            "    \"state\": \"work_item\",\n",
            "    \"profile_id\": \"default\",\n",
            "    \"updated_at\": \"2026-02-24T10:00:00Z\",\n",
            "    \"terminal\": false\n",
            "  }\n",
            "}\n"
        ),
    )
    .expect("index event should be writable");
}

fn write_stale_precondition_events(root: &Path) {
    let stale_idx = root
        .join(".knots/index/2026/02/24/0301-idx.knot_head.json");
    std::fs::write(
        &stale_idx,
        concat!(
            "{\n",
            "  \"event_id\": \"0301\",\n",
            "  \"occurred_at\": \"2026-02-24T10:00:01Z\",\n",
            "  \"type\": \"idx.knot_head\",\n",
            "  \"data\": {\n",
            "    \"knot_id\": \"K-occ\",\n",
            "    \"title\": \"Stale title\",\n",
            "    \"state\": \"implementing\",\n",
            "    \"profile_id\": \"default\",\n",
            "    \"updated_at\": \"2026-02-24T10:00:01Z\",\n",
            "    \"terminal\": false\n",
            "  },\n",
            "  \"precondition\": ",
            "{\"profile_etag\": \"missing-etag\"}\n",
            "}\n"
        ),
    )
    .expect("stale index event should be writable");

    let stale_full = root.join(
        ".knots/events/2026/02/24/0302-knot.description_set.json",
    );
    std::fs::create_dir_all(
        stale_full.parent().expect("full event parent"),
    )
    .expect("full event dir should be creatable");
    std::fs::write(
        &stale_full,
        concat!(
            "{\n",
            "  \"event_id\": \"0302\",\n",
            "  \"occurred_at\": \"2026-02-24T10:00:02Z\",\n",
            "  \"knot_id\": \"K-occ\",\n",
            "  \"type\": \"knot.description_set\",\n",
            "  \"data\": ",
            "{\"description\": \"stale description\"},\n",
            "  \"precondition\": ",
            "{\"profile_etag\": \"missing-etag\"}\n",
            "}\n"
        ),
    )
    .expect("stale full event should be writable");
}

#[test]
fn sync_ignores_events_with_stale_preconditions() {
    let root = unique_workspace();
    init_repo(&root);
    run_git(&root, &["checkout", "-b", "knots"]);

    write_stale_index_event(&root);
    write_stale_precondition_events(&root);

    run_git(&root, &["add", ".knots"]);
    run_git(
        &root,
        &["commit", "-m", "seed stale precondition events"],
    );
    run_git(&root, &["checkout", "main"]);

    let conn = open_sync_db(&root);
    let service = SyncService::new(&conn, root.clone());
    let _ = service.sync().expect("sync should succeed");

    let knot = db::get_knot_hot(&conn, "K-occ")
        .expect("knot query should succeed")
        .expect("knot should exist");
    assert_eq!(knot.title, "Original title");
    assert_eq!(knot.state, "work_item");
    assert_eq!(knot.profile_id, "default");
    assert_eq!(knot.description, None);
    assert_eq!(knot.profile_etag.as_deref(), Some("0300"));

    let _ = std::fs::remove_dir_all(root);
}

fn write_snapshot_files(root: &Path) {
    let snapshots_dir = root.join(".knots").join("snapshots");
    std::fs::create_dir_all(&snapshots_dir)
        .expect("snapshot dir should be creatable");
    let active_path = snapshots_dir
        .join("20260224T120000Z-active_catalog.snapshot.json");
    std::fs::write(
        &active_path,
        concat!(
            "{\n",
            "  \"schema_version\": 1,\n",
            "  \"written_at\": \"2026-02-24T12:00:00Z\",\n",
            "  \"hot\": [\n",
            "    {\n",
            "      \"id\": \"K-snap\",\n",
            "      \"title\": \"Snapshot knot\",\n",
            "      \"state\": \"work_item\",\n",
            "      \"updated_at\": ",
            "\"2026-02-24T12:00:00Z\",\n",
            "      \"body\": \"snapshot body\",\n",
            "      \"description\": \"snapshot body\",\n",
            "      \"priority\": 1,\n",
            "      \"knot_type\": \"task\",\n",
            "      \"tags\": [\"snapshot\"],\n",
            "      \"notes\": [],\n",
            "      \"handoff_capsules\": [],\n",
            "      \"profile_etag\": \"snap-1\",\n",
            "      \"profile_id\": \"default\",\n",
            "      \"created_at\": ",
            "\"2026-02-24T12:00:00Z\"\n",
            "    }\n",
            "  ],\n",
            "  \"warm\": []\n",
            "}\n"
        ),
    )
    .expect("active snapshot should be writable");
    let cold_path = snapshots_dir
        .join("20260224T120000Z-cold_catalog.snapshot.json");
    std::fs::write(
        &cold_path,
        concat!(
            "{\n",
            "  \"schema_version\": 1,\n",
            "  \"written_at\": \"2026-02-24T12:00:00Z\",\n",
            "  \"cold\": []\n",
            "}\n"
        ),
    )
    .expect("cold snapshot should be writable");
}

#[test]
fn sync_bootstrap_loads_latest_snapshots_when_no_events() {
    let root = unique_workspace();
    init_repo(&root);
    run_git(&root, &["checkout", "-b", "knots"]);

    write_snapshot_files(&root);

    run_git(&root, &["add", ".knots"]);
    run_git(&root, &["commit", "-m", "seed snapshots"]);
    run_git(&root, &["checkout", "main"]);

    let conn = open_sync_db(&root);
    let service = SyncService::new(&conn, root.clone());
    let summary = service.sync().expect("sync should succeed");
    assert_eq!(summary.index_files, 0);
    assert_eq!(summary.full_files, 0);

    let knot = db::get_knot_hot(&conn, "K-snap")
        .expect("knot query should succeed")
        .expect("snapshot knot should be loaded");
    assert_eq!(knot.title, "Snapshot knot");

    let _ = std::fs::remove_dir_all(root);
}
