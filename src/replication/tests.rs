use std::path::{Path, PathBuf};
use std::process::Command;

use uuid::Uuid;

use crate::db;
use crate::remote_init::init_remote_knots_branch;
use crate::sync::SyncError;

use super::ReplicationService;

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-repl-test-{}", Uuid::now_v7()));
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

fn setup_origin_and_dev1(root: &Path) -> (PathBuf, PathBuf) {
    let origin = root.join("origin.git");
    let dev1 = root.join("dev1");

    run_git(
        root,
        &["init", "--bare", origin.to_str().expect("utf8 path")],
    );
    std::fs::create_dir_all(&dev1).expect("dev1 dir should be creatable");
    run_git(&dev1, &["init"]);
    run_git(&dev1, &["config", "user.email", "knots@example.com"]);
    run_git(&dev1, &["config", "user.name", "Knots Test"]);

    std::fs::write(dev1.join("README.md"), "# knots\n").expect("readme should be writable");
    std::fs::write(dev1.join(".gitignore"), "/.knots/\n").expect(".gitignore should be writable");
    run_git(&dev1, &["add", "README.md", ".gitignore"]);
    run_git(&dev1, &["commit", "-m", "init"]);
    run_git(&dev1, &["branch", "-M", "main"]);
    run_git(
        &dev1,
        &[
            "remote",
            "add",
            "origin",
            origin.to_str().expect("utf8 path"),
        ],
    );
    run_git(&dev1, &["push", "-u", "origin", "main"]);
    let output = Command::new("git")
        .arg("--git-dir")
        .arg(&origin)
        .args(["symbolic-ref", "HEAD", "refs/heads/main"])
        .output()
        .expect("git symbolic-ref should run");
    assert!(
        output.status.success(),
        "git symbolic-ref failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    (origin, dev1)
}

fn setup_repo_without_remote(root: &Path) -> PathBuf {
    let local = root.join("local-no-remote");
    std::fs::create_dir_all(&local).expect("local repo should be creatable");
    run_git(&local, &["init"]);
    run_git(&local, &["config", "user.email", "knots@example.com"]);
    run_git(&local, &["config", "user.name", "Knots Test"]);
    std::fs::write(local.join("README.md"), "# knots\n").expect("readme should be writable");
    std::fs::write(local.join(".gitignore"), "/.knots/\n").expect(".gitignore should write");
    run_git(&local, &["add", "README.md", ".gitignore"]);
    run_git(&local, &["commit", "-m", "init"]);
    run_git(&local, &["branch", "-M", "main"]);
    local
}

fn write_local_knot_events(repo_root: &Path) {
    let idx_path = repo_root
        .join(".knots")
        .join("index")
        .join("2026")
        .join("02")
        .join("24")
        .join("9001-idx.knot_head.json");
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
            "  \"event_id\": \"9001\",\n",
            "  \"occurred_at\": \"2026-02-24T10:00:00Z\",\n",
            "  \"type\": \"idx.knot_head\",\n",
            "  \"data\": {\n",
            "    \"knot_id\": \"K-publish\",\n",
            "    \"title\": \"Published knot\",\n",
            "    \"state\": \"work_item\",\n",
            "    \"workflow_id\": \"default\",\n",
            "    \"updated_at\": \"2026-02-24T10:00:00Z\",\n",
            "    \"terminal\": false\n",
            "  }\n",
            "}\n"
        ),
    )
    .expect("index event should be writable");

    let full_path = repo_root
        .join(".knots")
        .join("events")
        .join("2026")
        .join("02")
        .join("24")
        .join("9002-knot.description_set.json");
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
            "  \"event_id\": \"9002\",\n",
            "  \"occurred_at\": \"2026-02-24T10:00:01Z\",\n",
            "  \"knot_id\": \"K-publish\",\n",
            "  \"type\": \"knot.description_set\",\n",
            "  \"data\": {\"description\": \"published details\"}\n",
            "}\n"
        ),
    )
    .expect("full event should be writable");
}

fn write_conflicting_local_index(repo_root: &Path) {
    let idx_path = repo_root
        .join(".knots")
        .join("index")
        .join("2026")
        .join("02")
        .join("24")
        .join("9001-idx.knot_head.json");
    std::fs::write(
        idx_path,
        concat!(
            "{\n",
            "  \"event_id\": \"9001\",\n",
            "  \"occurred_at\": \"2026-02-24T10:00:00Z\",\n",
            "  \"type\": \"idx.knot_head\",\n",
            "  \"data\": {\n",
            "    \"knot_id\": \"K-publish\",\n",
            "    \"title\": \"Locally changed title\",\n",
            "    \"state\": \"work_item\",\n",
            "    \"workflow_id\": \"automation_granular\",\n",
            "    \"updated_at\": \"2026-02-24T10:00:00Z\",\n",
            "    \"terminal\": false\n",
            "  }\n",
            "}\n"
        ),
    )
    .expect("conflicting index event should be writable");
}

#[test]
fn push_then_pull_shares_knots_between_clones() {
    let root = unique_workspace();
    let (origin, dev1) = setup_origin_and_dev1(&root);

    write_local_knot_events(&dev1);
    init_remote_knots_branch(&dev1).expect("remote knots branch should initialize");

    let db1_path = dev1.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(db1_path.parent().expect("dev1 db parent should exist"))
        .expect("dev1 db parent should be creatable");
    let conn1 =
        db::open_connection(db1_path.to_str().expect("utf8 path")).expect("dev1 db should open");
    let service1 = ReplicationService::new(&conn1, dev1.clone());
    let push = service1.push().expect("push should succeed");
    assert!(push.pushed);

    let dev2 = root.join("dev2");
    run_git(
        &root,
        &[
            "clone",
            origin.to_str().expect("utf8 path"),
            dev2.to_str().expect("utf8 path"),
        ],
    );
    run_git(&dev2, &["config", "user.email", "knots@example.com"]);
    run_git(&dev2, &["config", "user.name", "Knots Test"]);

    let db2_path = dev2.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(db2_path.parent().expect("dev2 db parent should exist"))
        .expect("dev2 db parent should be creatable");
    let conn2 =
        db::open_connection(db2_path.to_str().expect("utf8 path")).expect("dev2 db should open");
    let service2 = ReplicationService::new(&conn2, dev2.clone());
    let pull = service2.pull().expect("pull should succeed");
    assert!(pull.index_files >= 1);

    let knot = db::get_knot_hot(&conn2, "K-publish")
        .expect("knot query should succeed")
        .expect("knot should be present after pull");
    assert_eq!(knot.title, "Published knot");
    assert_eq!(knot.description.as_deref(), Some("published details"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn push_returns_noop_when_no_local_event_files_exist() {
    let root = unique_workspace();
    let (_origin, dev1) = setup_origin_and_dev1(&root);
    init_remote_knots_branch(&dev1).expect("remote knots branch should initialize");

    let db_path = dev1.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(db_path.parent().expect("db parent should exist"))
        .expect("db parent should be creatable");
    let conn = db::open_connection(db_path.to_str().expect("utf8 path")).expect("db should open");
    let service = ReplicationService::new(&conn, dev1.clone());

    let summary = service.push().expect("push should succeed");
    assert_eq!(summary.local_event_files, 0);
    assert_eq!(summary.copied_files, 0);
    assert!(!summary.committed);
    assert!(!summary.pushed);
    assert!(summary.commit.is_none());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn second_push_is_noop_when_remote_already_matches_local_events() {
    let root = unique_workspace();
    let (_origin, dev1) = setup_origin_and_dev1(&root);
    write_local_knot_events(&dev1);
    init_remote_knots_branch(&dev1).expect("remote knots branch should initialize");

    let db_path = dev1.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(db_path.parent().expect("db parent should exist"))
        .expect("db parent should be creatable");
    let conn = db::open_connection(db_path.to_str().expect("utf8 path")).expect("db should open");
    let service = ReplicationService::new(&conn, dev1.clone());

    let first = service.push().expect("initial push should succeed");
    assert!(first.pushed);
    let second = service.push().expect("second push should succeed");
    assert!(!second.committed);
    assert!(!second.pushed);
    assert!(second.commit.is_none());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn push_reports_conflict_when_remote_file_content_differs() {
    let root = unique_workspace();
    let (_origin, dev1) = setup_origin_and_dev1(&root);
    write_local_knot_events(&dev1);
    init_remote_knots_branch(&dev1).expect("remote knots branch should initialize");

    let db_path = dev1.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(db_path.parent().expect("db parent should exist"))
        .expect("db parent should be creatable");
    let conn = db::open_connection(db_path.to_str().expect("utf8 path")).expect("db should open");
    let service = ReplicationService::new(&conn, dev1.clone());

    let first = service.push().expect("initial push should succeed");
    assert!(first.pushed);

    write_conflicting_local_index(&dev1);
    let err = service.push().expect_err("conflicting push should fail");
    assert!(matches!(err, SyncError::FileConflict { .. }));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn push_propagates_missing_remote_errors_after_local_reset_fallback() {
    let root = unique_workspace();
    let local = setup_repo_without_remote(&root);
    write_local_knot_events(&local);

    let db_path = local.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(db_path.parent().expect("db parent should exist"))
        .expect("db parent should be creatable");
    let conn = db::open_connection(db_path.to_str().expect("utf8 path")).expect("db should open");
    let service = ReplicationService::new(&conn, local.clone());

    let err = service
        .push()
        .expect_err("push should fail without configured remote");
    assert!(matches!(err, SyncError::GitCommandFailed { .. }));

    let _ = std::fs::remove_dir_all(root);
}
