use std::path::{Path, PathBuf};
use std::process::Command;
use uuid::Uuid;

use crate::db;

use super::{GitAdapter, KnotsWorktree, SyncService};

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-sync-test-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    root
}

fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
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

    std::fs::write(root.join("README.md"), "# test\n").expect("readme should be writable");
    run_git(root, &["add", "README.md"]);
    run_git(root, &["commit", "-m", "init"]);
    run_git(root, &["branch", "-M", "main"]);
}

#[test]
fn worktree_manager_creates_knots_branch_worktree() {
    let root = unique_workspace();
    init_repo(&root);

    let git = GitAdapter::new();
    let worktree = KnotsWorktree::new(root.clone());
    worktree
        .ensure_exists(&git)
        .expect("worktree should be created");

    assert!(worktree.path().join(".git").exists());
    let branch = git
        .current_branch(worktree.path())
        .expect("current branch should be available");
    assert_eq!(branch, "knots");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn sync_applies_index_and_edge_events_from_knots_branch() {
    let root = unique_workspace();
    init_repo(&root);

    run_git(&root, &["checkout", "-b", "knots"]);

    let idx_path = root
        .join(".knots")
        .join("index")
        .join("2026")
        .join("02")
        .join("22")
        .join("0001-idx.knot_head.json");
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
            "  \"event_id\": \"0001\",\n",
            "  \"occurred_at\": \"2026-02-22T10:00:00Z\",\n",
            "  \"type\": \"idx.knot_head\",\n",
            "  \"data\": {\n",
            "    \"knot_id\": \"K-1\",\n",
            "    \"title\": \"Synced knot\",\n",
            "    \"state\": \"work_item\",\n",
            "    \"updated_at\": \"2026-02-22T10:00:00Z\",\n",
            "    \"terminal\": false\n",
            "  }\n",
            "}\n"
        ),
    )
    .expect("index event should be writable");

    let full_path = root
        .join(".knots")
        .join("events")
        .join("2026")
        .join("02")
        .join("22")
        .join("0002-knot.edge_add.json");
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
            "  \"event_id\": \"0002\",\n",
            "  \"occurred_at\": \"2026-02-22T10:00:01Z\",\n",
            "  \"knot_id\": \"K-1\",\n",
            "  \"type\": \"knot.edge_add\",\n",
            "  \"data\": {\n",
            "    \"kind\": \"blocked_by\",\n",
            "    \"dst\": \"K-2\"\n",
            "  }\n",
            "}\n"
        ),
    )
    .expect("full event should be writable");

    run_git(&root, &["add", ".knots"]);
    run_git(&root, &["commit", "-m", "seed knots events"]);
    run_git(&root, &["checkout", "main"]);

    let db_path = root.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(
        db_path
            .parent()
            .expect("db parent should exist for sync test"),
    )
    .expect("db parent should be creatable");
    let conn = db::open_connection(db_path.to_str().expect("utf8 path"))
        .expect("sync test database should open");

    let service = SyncService::new(&conn, root.clone());
    let summary = service.sync().expect("sync should succeed");
    assert_eq!(summary.index_files, 1);
    assert_eq!(summary.full_files, 1);
    assert_eq!(summary.knot_updates, 1);
    assert_eq!(summary.edge_adds, 1);

    let knot = db::get_knot_hot(&conn, "K-1")
        .expect("knot query should succeed")
        .expect("knot should be present in hot cache");
    assert_eq!(knot.title, "Synced knot");

    let edges = db::list_edges(&conn, "K-1", db::EdgeDirection::Outgoing)
        .expect("edge list should succeed");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].dst, "K-2");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn sync_reduces_description_tag_and_note_events() {
    let root = unique_workspace();
    init_repo(&root);
    run_git(&root, &["checkout", "-b", "knots"]);

    let idx_path = root
        .join(".knots")
        .join("index")
        .join("2026")
        .join("02")
        .join("23")
        .join("0100-idx.knot_head.json");
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
            "  \"event_id\": \"0100\",\n",
            "  \"occurred_at\": \"2026-02-23T10:00:00Z\",\n",
            "  \"type\": \"idx.knot_head\",\n",
            "  \"data\": {\n",
            "    \"knot_id\": \"K-7\",\n",
            "    \"title\": \"Sync parity\",\n",
            "    \"state\": \"work_item\",\n",
            "    \"updated_at\": \"2026-02-23T10:00:00Z\",\n",
            "    \"terminal\": false\n",
            "  }\n",
            "}\n"
        ),
    )
    .expect("index event should be writable");

    let desc_path = root
        .join(".knots")
        .join("events")
        .join("2026")
        .join("02")
        .join("23")
        .join("0101-knot.description_set.json");
    std::fs::create_dir_all(
        desc_path
            .parent()
            .expect("description event parent directory should exist"),
    )
    .expect("description event directory should be creatable");
    std::fs::write(
        &desc_path,
        concat!(
            "{\n",
            "  \"event_id\": \"0101\",\n",
            "  \"occurred_at\": \"2026-02-23T10:01:00Z\",\n",
            "  \"knot_id\": \"K-7\",\n",
            "  \"type\": \"knot.description_set\",\n",
            "  \"data\": {\"description\": \"synced description\"}\n",
            "}\n"
        ),
    )
    .expect("description event should be writable");

    let tag_path = root
        .join(".knots")
        .join("events")
        .join("2026")
        .join("02")
        .join("23")
        .join("0102-knot.tag_add.json");
    std::fs::write(
        &tag_path,
        concat!(
            "{\n",
            "  \"event_id\": \"0102\",\n",
            "  \"occurred_at\": \"2026-02-23T10:02:00Z\",\n",
            "  \"knot_id\": \"K-7\",\n",
            "  \"type\": \"knot.tag_add\",\n",
            "  \"data\": {\"tag\": \"migration\"}\n",
            "}\n"
        ),
    )
    .expect("tag event should be writable");

    let note_path = root
        .join(".knots")
        .join("events")
        .join("2026")
        .join("02")
        .join("23")
        .join("0103-knot.note_added.json");
    std::fs::write(
        &note_path,
        concat!(
            "{\n",
            "  \"event_id\": \"0103\",\n",
            "  \"occurred_at\": \"2026-02-23T10:03:00Z\",\n",
            "  \"knot_id\": \"K-7\",\n",
            "  \"type\": \"knot.note_added\",\n",
            "  \"data\": {\n",
            "    \"entry_id\": \"note-1\",\n",
            "    \"content\": \"synced note\",\n",
            "    \"username\": \"acartine\",\n",
            "    \"datetime\": \"2026-02-23T10:03:00Z\",\n",
            "    \"agentname\": \"codex\",\n",
            "    \"model\": \"gpt-5\",\n",
            "    \"version\": \"0.1\"\n",
            "  }\n",
            "}\n"
        ),
    )
    .expect("note event should be writable");

    run_git(&root, &["add", ".knots"]);
    run_git(&root, &["commit", "-m", "seed parity full events"]);
    run_git(&root, &["checkout", "main"]);

    let db_path = root.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(
        db_path
            .parent()
            .expect("db parent should exist for sync test"),
    )
    .expect("db parent should be creatable");
    let conn = db::open_connection(db_path.to_str().expect("utf8 path"))
        .expect("sync test database should open");

    let service = SyncService::new(&conn, root.clone());
    let summary = service.sync().expect("sync should succeed");
    assert_eq!(summary.index_files, 1);
    assert_eq!(summary.full_files, 3);

    let knot = db::get_knot_hot(&conn, "K-7")
        .expect("knot query should succeed")
        .expect("knot should be present in hot cache");
    assert_eq!(knot.description.as_deref(), Some("synced description"));
    assert!(knot.tags.contains(&"migration".to_string()));
    assert_eq!(knot.notes.len(), 1);
    assert_eq!(knot.notes[0].entry_id, "note-1");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn sync_classifies_old_knots_as_warm_and_terminal_as_cold() {
    let root = unique_workspace();
    init_repo(&root);
    run_git(&root, &["checkout", "-b", "knots"]);

    let warm_idx = root
        .join(".knots")
        .join("index")
        .join("2025")
        .join("01")
        .join("01")
        .join("0200-idx.knot_head.json");
    std::fs::create_dir_all(
        warm_idx
            .parent()
            .expect("warm index event parent directory should exist"),
    )
    .expect("warm index event directory should be creatable");
    std::fs::write(
        &warm_idx,
        concat!(
            "{\n",
            "  \"event_id\": \"0200\",\n",
            "  \"occurred_at\": \"2025-01-01T00:00:00Z\",\n",
            "  \"type\": \"idx.knot_head\",\n",
            "  \"data\": {\n",
            "    \"knot_id\": \"K-warm\",\n",
            "    \"title\": \"Warm candidate\",\n",
            "    \"state\": \"work_item\",\n",
            "    \"updated_at\": \"2025-01-01T00:00:00Z\",\n",
            "    \"terminal\": false\n",
            "  }\n",
            "}\n"
        ),
    )
    .expect("warm index event should be writable");

    let cold_idx = root
        .join(".knots")
        .join("index")
        .join("2026")
        .join("02")
        .join("23")
        .join("0201-idx.knot_head.json");
    std::fs::create_dir_all(
        cold_idx
            .parent()
            .expect("cold index event parent directory should exist"),
    )
    .expect("cold index event directory should be creatable");
    std::fs::write(
        &cold_idx,
        concat!(
            "{\n",
            "  \"event_id\": \"0201\",\n",
            "  \"occurred_at\": \"2026-02-23T00:00:00Z\",\n",
            "  \"type\": \"idx.knot_head\",\n",
            "  \"data\": {\n",
            "    \"knot_id\": \"K-cold\",\n",
            "    \"title\": \"Cold candidate\",\n",
            "    \"state\": \"shipped\",\n",
            "    \"updated_at\": \"2026-02-23T00:00:00Z\",\n",
            "    \"terminal\": true\n",
            "  }\n",
            "}\n"
        ),
    )
    .expect("cold index event should be writable");

    run_git(&root, &["add", ".knots"]);
    run_git(&root, &["commit", "-m", "seed warm and cold"]);
    run_git(&root, &["checkout", "main"]);

    let db_path = root.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(
        db_path
            .parent()
            .expect("db parent should exist for sync test"),
    )
    .expect("db parent should be creatable");
    let conn = db::open_connection(db_path.to_str().expect("utf8 path"))
        .expect("sync test database should open");

    let service = SyncService::new(&conn, root.clone());
    let summary = service.sync().expect("sync should succeed");
    assert_eq!(summary.index_files, 2);

    let hot_warm = db::get_knot_hot(&conn, "K-warm").expect("hot lookup should succeed");
    assert!(hot_warm.is_none());
    let warm = db::get_knot_warm(&conn, "K-warm").expect("warm lookup should succeed");
    assert_eq!(
        warm.expect("warm entry should exist").title,
        "Warm candidate"
    );

    let cold = db::get_cold_catalog(&conn, "K-cold").expect("cold lookup should succeed");
    let cold = cold.expect("cold entry should exist");
    assert_eq!(cold.state, "shipped");

    let _ = std::fs::remove_dir_all(root);
}
