use std::path::{Path, PathBuf};
use std::process::Command;

use crate::app::AppError;
use crate::cli_sync_ref::{SyncRefArgs, SyncRefMigrateArgs, SyncRefSubcommands};

use super::{migrate, run_sync_ref_command};

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "knots-sync-ref-migrate-test-{}",
        uuid::Uuid::now_v7()
    ));
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

fn git_stdout(root: &Path, args: &[&str]) -> String {
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
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn setup_repo() -> (PathBuf, PathBuf) {
    let root = unique_workspace();
    let remote = root.join("origin.git");
    let repo = root.join("repo");
    run_git(&root, &["init", "--bare", path(&remote)]);
    std::fs::create_dir_all(&repo).expect("repo dir should be creatable");
    run_git(&repo, &["init"]);
    run_git(&repo, &["config", "user.email", "knots@example.com"]);
    run_git(&repo, &["config", "user.name", "Knots Test"]);
    std::fs::write(repo.join("README.md"), "# test\n").expect("readme should write");
    run_git(&repo, &["add", "README.md"]);
    run_git(&repo, &["commit", "-m", "init"]);
    run_git(&repo, &["branch", "-M", "main"]);
    run_git(&repo, &["remote", "add", "origin", path(&remote)]);
    run_git(&repo, &["push", "-u", "origin", "HEAD:refs/heads/main"]);
    (root, repo)
}

fn write_remote_knots_ref(repo: &Path, refname: &str, file: &str, contents: &str) {
    run_git(repo, &["checkout", "--orphan", "knots-source"]);
    let _ = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rm", "-r", "--ignore-unmatch", "."])
        .output()
        .expect("git rm should run");
    let target = repo.join(file);
    std::fs::create_dir_all(target.parent().expect("file should have parent"))
        .expect("parent should be creatable");
    std::fs::write(&target, contents).expect("remote fixture file should write");
    run_git(repo, &["add", "-f", file]);
    run_git(repo, &["commit", "-m", "knots source"]);
    run_git(repo, &["push", "origin", &format!("HEAD:{refname}")]);
    run_git(repo, &["checkout", "main"]);
    run_git(repo, &["branch", "-D", "knots-source"]);
}

fn write_local_knots_file(repo: &Path, file: &str, contents: &str) {
    let target = repo.join(file);
    std::fs::create_dir_all(target.parent().expect("file should have parent"))
        .expect("parent should be creatable");
    std::fs::write(target, contents).expect("local fixture file should write");
}

#[test]
fn migrate_unions_remote_and_local_knots_files_into_work_ref() {
    let (root, repo) = setup_repo();
    write_remote_knots_ref(
        &repo,
        "refs/heads/knots",
        ".knots/index/2026/06/12/remote.json",
        "{\"event_id\":\"remote\"}\n",
    );
    write_local_knots_file(
        &repo,
        ".knots/index/2026/06/12/remote.json",
        "{\"event_id\":\"remote\"}\n",
    );
    write_local_knots_file(
        &repo,
        ".knots/events/2026/06/12/local.json",
        "{\"event_id\":\"local\"}\n",
    );
    write_local_knots_file(&repo, ".knots/events/2026/06/12/ignore.txt", "ignored\n");

    let summary = migrate(
        &repo,
        &["origin:refs/heads/knots".to_string(), "local".to_string()],
        Some("origin:refs/work/knots"),
    )
    .expect("migration should succeed");
    assert_eq!(summary.files, 2);
    assert_eq!(summary.target.refname, "refs/work/knots");

    run_git(&repo, &["fetch", "origin", "refs/work/knots"]);
    let remote = git_stdout(
        &repo,
        &["show", "FETCH_HEAD:.knots/index/2026/06/12/remote.json"],
    );
    let local = git_stdout(
        &repo,
        &["show", "FETCH_HEAD:.knots/events/2026/06/12/local.json"],
    );
    assert_eq!(remote, "{\"event_id\":\"remote\"}");
    assert_eq!(local, "{\"event_id\":\"local\"}");
    assert_eq!(
        git_stdout(&repo, &["config", "--get", "knots.remoteRef"]),
        "refs/work/knots"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn migrate_default_target_uses_config_and_returns_unchanged_on_second_run() {
    let (root, repo) = setup_repo();
    write_local_knots_file(
        &repo,
        ".knots/events/2026/06/12/local.json",
        "{\"event_id\":\"local\"}\n",
    );

    let first = migrate(&repo, &["local".to_string()], None)
        .expect("default-target migration should succeed");
    assert_eq!(first.target.refname, "refs/heads/knots");
    assert!(first.commit.is_some());

    let second = migrate(&repo, &["local".to_string()], None)
        .expect("second migration should reuse existing target ref");
    assert_eq!(second.files, 1);
    assert_eq!(second.commit, None);

    run_git(&repo, &["fetch", "origin", "refs/heads/knots"]);
    assert_eq!(
        git_stdout(
            &repo,
            &["show", "FETCH_HEAD:.knots/events/2026/06/12/local.json"],
        ),
        "{\"event_id\":\"local\"}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn migrate_errors_on_empty_inputs_and_malformed_endpoints() {
    let (root, repo) = setup_repo();

    let empty = migrate(&repo, &[], Some("origin:refs/work/knots"))
        .expect_err("empty migration should fail");
    assert_invalid_argument_contains(empty, "no Knots event");

    let missing_colon = migrate(
        &repo,
        &["origin-knots".to_string()],
        Some("origin:refs/work/knots"),
    )
    .expect_err("malformed source should fail");
    assert_invalid_argument_contains(missing_colon, "expected '<remote>:<ref>'");

    let empty_ref = migrate(&repo, &["local".to_string()], Some("origin:"))
        .expect_err("malformed target should fail");
    assert_invalid_argument_contains(empty_ref, "expected '<remote>:<ref>'");

    let not_repo = root.join("not-a-repo");
    std::fs::create_dir_all(&not_repo).expect("non-repo dir should be creatable");
    let err = migrate(
        &not_repo,
        &["local".to_string()],
        Some("origin:refs/work/knots"),
    )
    .expect_err("non-git repo should fail");
    assert_invalid_argument_contains(err, "is not a git repository");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn sync_ref_command_wrapper_runs_migration() {
    let (root, repo) = setup_repo();
    write_local_knots_file(
        &repo,
        ".knots/snapshots/2026/06/12/snapshot.json",
        "{\"snapshot\":\"local\"}\n",
    );

    let args = SyncRefArgs {
        command: SyncRefSubcommands::Migrate(SyncRefMigrateArgs {
            sources: vec!["local".to_string()],
            target: Some("origin:refs/work/knots".to_string()),
        }),
    };
    run_sync_ref_command(&repo, &args).expect("command wrapper should migrate");

    run_git(&repo, &["fetch", "origin", "refs/work/knots"]);
    assert_eq!(
        git_stdout(
            &repo,
            &[
                "show",
                "FETCH_HEAD:.knots/snapshots/2026/06/12/snapshot.json"
            ],
        ),
        "{\"snapshot\":\"local\"}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn migrate_stops_on_same_path_different_content_conflict() {
    let (root, repo) = setup_repo();
    let file = ".knots/index/2026/06/12/same.json";
    write_remote_knots_ref(
        &repo,
        "refs/heads/knots",
        file,
        "{\"event_id\":\"remote\"}\n",
    );
    write_local_knots_file(&repo, file, "{\"event_id\":\"local\"}\n");

    let err = migrate(
        &repo,
        &["origin:refs/heads/knots".to_string(), "local".to_string()],
        Some("origin:refs/work/knots"),
    )
    .expect_err("migration should reject conflicting files");
    match err {
        AppError::InvalidArgument(message) => assert!(message.contains(file)),
        other => panic!("expected invalid argument, got {other:?}"),
    }

    let _ = std::fs::remove_dir_all(root);
}

fn assert_invalid_argument_contains(err: AppError, expected: &str) {
    match err {
        AppError::InvalidArgument(message) => assert!(
            message.contains(expected),
            "expected {message:?} to contain {expected:?}"
        ),
        other => panic!("expected invalid argument, got {other:?}"),
    }
}

fn path(path: &Path) -> &str {
    path.to_str().expect("path should be utf8")
}
