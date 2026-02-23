use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::db;

use super::{GitAdapter, KnotsWorktree, SyncService};

fn unique_workspace() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX_EPOCH")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("knots-sync-test-{}", nanos));
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
