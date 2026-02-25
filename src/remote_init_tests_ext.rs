use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{
    detect_beads_hooks, init_remote_branch, init_remote_knots_branch, remote_branch_exists,
    should_retry_push_without_verify, uninit_remote_knots_branch, RemoteInitError,
};

fn unique_dir(prefix: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("{}-{}", prefix, uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&path).expect("temp dir should be creatable");
    path
}

fn run_git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
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

fn setup_repo_with_remote() -> (PathBuf, PathBuf) {
    let root = unique_dir("knots-remote-init-ext");
    let remote = root.join("remote.git");
    let local = root.join("local");

    std::fs::create_dir_all(&local).expect("local dir should be creatable");
    run_git(
        &root,
        &["init", "--bare", remote.to_str().expect("utf8 path")],
    );
    run_git(&local, &["init"]);
    run_git(&local, &["config", "user.email", "knots@example.com"]);
    run_git(&local, &["config", "user.name", "Knots Test"]);
    std::fs::write(local.join("README.md"), "# knots\n").expect("readme should be writable");
    run_git(&local, &["add", "README.md"]);
    run_git(&local, &["commit", "-m", "init"]);
    run_git(
        &local,
        &[
            "remote",
            "add",
            "origin",
            remote.to_str().expect("utf8 path"),
        ],
    );

    (root, local)
}

#[test]
fn remote_init_error_display_and_source_cover_variants() {
    let not_repo = RemoteInitError::NotGitRepository;
    assert!(not_repo.to_string().contains("not a git repository"));
    assert!(not_repo.source().is_none());

    let missing = RemoteInitError::MissingRemote("origin".to_string());
    assert!(missing.to_string().contains("is not configured"));
    assert!(missing.source().is_none());

    let exists = RemoteInitError::RemoteBranchExists {
        remote: "origin".to_string(),
        branch: "knots".to_string(),
    };
    assert!(exists.to_string().contains("already exists"));
    assert!(exists.source().is_none());

    let command_failed = RemoteInitError::GitCommandFailed {
        command: "git ls-remote".to_string(),
        code: Some(1),
        stderr: "fatal".to_string(),
    };
    assert!(command_failed.to_string().contains("git command failed"));
    assert!(command_failed.source().is_none());

    let io = RemoteInitError::Io(std::io::Error::other("disk"));
    assert!(io.to_string().contains("I/O error"));
    assert!(io.source().is_some());
}

#[test]
fn remote_branch_exists_and_uninit_cover_present_and_missing_paths() {
    let (root, local) = setup_repo_with_remote();

    let missing = remote_branch_exists(&local, "origin", "knots")
        .expect("missing branch check should succeed");
    assert!(!missing);

    init_remote_knots_branch(&local).expect("remote init should succeed");
    let present = remote_branch_exists(&local, "origin", "knots")
        .expect("present branch check should succeed");
    assert!(present);

    let deleted = uninit_remote_knots_branch(&local, "origin", "knots")
        .expect("uninit should delete existing branch");
    assert!(deleted);

    let already_missing = uninit_remote_knots_branch(&local, "origin", "knots")
        .expect("uninit should succeed when branch is missing");
    assert!(!already_missing);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn uninit_reports_not_repo_or_missing_remote_and_hooks_path_is_respected() {
    let root = unique_dir("knots-remote-init-not-repo");
    assert!(matches!(
        uninit_remote_knots_branch(&root, "origin", "knots"),
        Err(RemoteInitError::NotGitRepository)
    ));
    let _ = std::fs::remove_dir_all(root);

    let repo = unique_dir("knots-remote-init-no-remote");
    run_git(&repo, &["init"]);
    run_git(&repo, &["config", "user.email", "knots@example.com"]);
    run_git(&repo, &["config", "user.name", "Knots Test"]);
    assert!(matches!(
        uninit_remote_knots_branch(&repo, "origin", "knots"),
        Err(RemoteInitError::MissingRemote(_))
    ));

    run_git(&repo, &["config", "core.hooksPath", ".custom-hooks"]);
    let hooks = repo.join(".custom-hooks");
    std::fs::create_dir_all(&hooks).expect("custom hooks dir should be creatable");
    let pre_push = hooks.join("pre-push");
    std::fs::write(&pre_push, "#!/bin/sh\nbeads sync\n").expect("hook should be writable");
    let report = detect_beads_hooks(&repo);
    assert!(!report.is_empty());
    assert!(report.hooks_dir.ends_with(".custom-hooks"));
    assert!(report.hook_files.contains(&pre_push));

    let _ = std::fs::remove_dir_all(repo);
}

#[test]
fn init_remote_branch_retries_push_without_verify_when_beads_hook_fails() {
    let (root, local) = setup_repo_with_remote();
    let hooks = local.join(".git").join("hooks");
    std::fs::create_dir_all(&hooks).expect("hooks dir should be creatable");
    let pre_push = hooks.join("pre-push");
    std::fs::write(
        &pre_push,
        "#!/bin/sh\necho 'beads pre-push check failed' >&2\nexit 1\n",
    )
    .expect("pre-push hook should be writable");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&pre_push)
            .expect("pre-push metadata should be readable")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&pre_push, perms).expect("pre-push permissions should be set");
    }

    init_remote_branch(&local, "origin", "knots")
        .expect("push should retry with --no-verify and succeed");

    let present = remote_branch_exists(&local, "origin", "knots")
        .expect("branch existence check should succeed");
    assert!(present);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn detect_beads_hooks_respects_absolute_core_hooks_path() {
    let (root, local) = setup_repo_with_remote();
    let absolute_hooks = root.join("custom-hooks");
    std::fs::create_dir_all(&absolute_hooks).expect("custom hooks should be creatable");
    run_git(
        &local,
        &[
            "config",
            "core.hooksPath",
            absolute_hooks.to_str().expect("utf8 hooks path"),
        ],
    );
    let pre_push = absolute_hooks.join("pre-push");
    std::fs::write(&pre_push, "#!/bin/sh\nbeads sync\n").expect("hook should be writable");
    let unreadable = absolute_hooks.join("commit-msg");
    std::fs::create_dir_all(&unreadable).expect("fixture directory should be creatable");

    let report = detect_beads_hooks(&local);
    assert_eq!(report.hooks_dir, absolute_hooks);
    assert!(report.hook_files.contains(&pre_push));
    assert!(!report.hook_files.contains(&unreadable));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn remote_branch_exists_returns_git_command_failure_for_unreachable_remote() {
    let root = unique_dir("knots-remote-exists-failure");
    run_git(&root, &["init"]);
    run_git(&root, &["config", "user.email", "knots@example.com"]);
    run_git(&root, &["config", "user.name", "Knots Test"]);
    std::fs::write(root.join("README.md"), "# test\n").expect("readme should be writable");
    run_git(&root, &["add", "README.md"]);
    run_git(&root, &["commit", "-m", "init"]);
    run_git(
        &root,
        &["remote", "add", "origin", "file:///no/such/remote/repo.git"],
    );

    let err =
        remote_branch_exists(&root, "origin", "knots").expect_err("unreachable remote should fail");
    assert!(matches!(err, RemoteInitError::GitCommandFailed { .. }));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn helper_paths_cover_io_conversion_and_non_git_detection() {
    let converted: RemoteInitError = std::io::Error::other("disk").into();
    assert!(matches!(converted, RemoteInitError::Io(_)));

    let non_git = unique_dir("knots-remote-non-git-init");
    let report = detect_beads_hooks(&non_git);
    assert!(report.is_empty());
    assert!(matches!(
        init_remote_branch(&non_git, "origin", "knots"),
        Err(RemoteInitError::NotGitRepository)
    ));
    let _ = std::fs::remove_dir_all(non_git);
}

#[test]
fn init_remote_branch_reports_unreachable_remote_as_git_command_failure() {
    let root = unique_dir("knots-remote-unreachable");
    run_git(&root, &["init"]);
    run_git(&root, &["config", "user.email", "knots@example.com"]);
    run_git(&root, &["config", "user.name", "Knots Test"]);
    std::fs::write(root.join("README.md"), "# test\n").expect("readme should be writable");
    run_git(&root, &["add", "README.md"]);
    run_git(&root, &["commit", "-m", "init"]);
    run_git(
        &root,
        &["remote", "add", "origin", "file:///no/such/remote/repo.git"],
    );

    let err = init_remote_branch(&root, "origin", "knots")
        .expect_err("unreachable remote should fail branch initialization");
    assert!(matches!(err, RemoteInitError::GitCommandFailed { .. }));

    let no_retry =
        should_retry_push_without_verify(&RemoteInitError::MissingRemote("origin".to_string()));
    assert!(!no_retry);

    let _ = std::fs::remove_dir_all(root);
}
