use std::path::{Path, PathBuf};
use std::process::Command;

use uuid::Uuid;

use crate::db;
use crate::remote_init::init_remote_knots_branch;
use crate::sync::SyncError;

use super::ReplicationService;

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-repl-policy-{}", Uuid::now_v7()));
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
    std::fs::write(dev1.join("README.md"), "# knots\n").expect("readme should write");
    std::fs::write(dev1.join(".gitignore"), "/.knots/\n").expect(".gitignore should write");
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
    (origin, dev1)
}

fn write_local_knot_events(repo_root: &Path) {
    let idx_path = repo_root.join(".knots/index/2026/02/24/9001-idx.knot_head.json");
    std::fs::create_dir_all(idx_path.parent().expect("index parent should exist"))
        .expect("index event directory should be creatable");
    std::fs::write(&idx_path, "{\"event_id\":\"9001\"}\n").expect("index event should write");

    let full_path = repo_root.join(".knots/events/2026/02/24/9002-knot.description_set.json");
    std::fs::create_dir_all(full_path.parent().expect("full parent should exist"))
        .expect("full event directory should be creatable");
    std::fs::write(&full_path, "{\"event_id\":\"9002\"}\n").expect("full event should write");
}

fn install_persona_ref_policy_hook(origin: &Path) {
    let hook = origin.join("hooks").join("pre-receive");
    std::fs::write(
        &hook,
        concat!(
            "#!/bin/sh\n",
            "while read old new refname; do\n",
            "  case \"$refname\" in\n",
            "    refs/work/*|refs/diffs/*|refs/notes/sneka-agents) ;;\n",
            "    *) echo \"diffinite: Agent Personas cannot push this ref\" >&2; exit 1 ;;\n",
            "  esac\n",
            "done\n"
        ),
    )
    .expect("pre-receive hook should be writable");
    make_executable(&hook);
}

fn install_always_non_fast_forward_hook(origin: &Path) {
    let hook = origin.join("hooks").join("pre-receive");
    std::fs::write(
        &hook,
        concat!(
            "#!/bin/sh\n",
            "echo hit >>\"$(pwd)/retry-count\"\n",
            "echo \"rejected non-fast-forward\" >&2\n",
            "exit 1\n"
        ),
    )
    .expect("pre-receive hook should be writable");
    make_executable(&hook);
}

fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)
            .expect("hook metadata should be readable")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).expect("hook permissions should be set");
    }
}

#[test]
fn push_to_work_ref_succeeds_with_diffinite_persona_policy() {
    let root = unique_workspace();
    let (origin, dev1) = setup_origin_and_dev1(&root);
    install_persona_ref_policy_hook(&origin);
    run_git(&dev1, &["config", "knots.remoteRef", "refs/work/knots"]);
    write_local_knot_events(&dev1);

    let conn = open_test_db(&dev1);
    let service = ReplicationService::new(&conn, dev1.clone());
    let summary = service.push().expect("work-ref push should succeed");
    assert!(summary.pushed);

    let output = Command::new("git")
        .arg("-C")
        .arg(&dev1)
        .args(["ls-remote", "origin", "refs/work/knots"])
        .output()
        .expect("git ls-remote should run");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("refs/work/knots"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn push_ref_policy_rejection_is_not_retried() {
    let root = unique_workspace();
    let (origin, dev1) = setup_origin_and_dev1(&root);
    run_git(&dev1, &["config", "knots.remoteRef", "refs/heads/knots"]);
    init_remote_knots_branch(&dev1).expect("legacy remote ref should initialize");
    install_persona_ref_policy_hook(&origin);
    write_local_knot_events(&dev1);

    let conn = open_test_db(&dev1);
    let service = ReplicationService::new(&conn, dev1.clone());
    let err = service
        .push()
        .expect_err("head-ref push should be rejected");
    let display = err.to_string();
    match err {
        SyncError::GitCommandFailed {
            command, stderr, ..
        } => {
            assert!(stderr.contains("Agent Personas cannot push this ref"));
            assert!(
                command.contains("refs/heads/knots") || display.contains("refs/heads/knots"),
                "error was: {display}"
            );
        }
        other => panic!("expected raw git policy failure, got {other:?}"),
    }

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn push_non_fast_forward_still_retries() {
    let root = unique_workspace();
    let (origin, dev1) = setup_origin_and_dev1(&root);
    init_remote_knots_branch(&dev1).expect("remote knots branch should initialize");
    install_always_non_fast_forward_hook(&origin);
    write_local_knot_events(&dev1);

    let conn = open_test_db(&dev1);
    let service = ReplicationService::new(&conn, dev1.clone());
    let err = service
        .push()
        .expect_err("persistent non-fast-forward should fail");
    assert!(
        matches!(err, SyncError::MergeConflictEscalation { .. }),
        "expected merge escalation, got {err:?}"
    );
    let retry_count = std::fs::read_to_string(origin.join("retry-count"))
        .expect("hook retry count should be written");
    assert_eq!(retry_count.lines().count(), 3);

    let _ = std::fs::remove_dir_all(root);
}

fn open_test_db(repo_root: &Path) -> rusqlite::Connection {
    let db_path = repo_root.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(db_path.parent().expect("db parent should exist"))
        .expect("db parent should be creatable");
    db::open_connection(db_path.to_str().expect("utf8 path")).expect("db should open")
}
