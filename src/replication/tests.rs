use std::path::{Path, PathBuf};
use std::process::Command;

use uuid::Uuid;

use crate::db;
use crate::remote_init::init_remote_knots_branch;

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

fn write_test_workflow_file(root: &Path) {
    let path = root.join(".knots").join("workflows.toml");
    std::fs::create_dir_all(
        path.parent()
            .expect("workflow file parent directory should exist"),
    )
    .expect("workflow directory should be creatable");
    std::fs::write(
        path,
        concat!(
            "[[workflows]]\n",
            "id = \"default\"\n",
            "initial_state = \"work_item\"\n",
            "states = [\"work_item\", \"implementing\", \"shipped\", \"abandoned\"]\n",
            "terminal_states = [\"shipped\", \"abandoned\"]\n",
            "\n",
            "[[workflows.transitions]]\n",
            "from = \"work_item\"\n",
            "to = \"implementing\"\n",
            "\n",
            "[[workflows.transitions]]\n",
            "from = \"implementing\"\n",
            "to = \"shipped\"\n",
            "\n",
            "[[workflows.transitions]]\n",
            "from = \"*\"\n",
            "to = \"abandoned\"\n"
        ),
    )
    .expect("workflow file should be writable");
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
    write_test_workflow_file(&dev1);

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
    write_test_workflow_file(&dev2);

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
