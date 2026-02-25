use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use uuid::Uuid;

fn unique_workspace(prefix: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&path).expect("workspace should be creatable");
    path
}

fn run_git(cwd: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .expect("git command should run")
}

fn run_script(cwd: &Path, script: &Path, args: &[&str]) -> Output {
    Command::new("bash")
        .arg(script)
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("script command should run")
}

fn init_repo(root: &Path) {
    assert!(run_git(root, &["init"]).status.success());
    assert!(
        run_git(root, &["config", "user.email", "knots@example.com"])
            .status
            .success()
    );
    assert!(run_git(root, &["config", "user.name", "Knots Test"])
        .status
        .success());
    std::fs::write(root.join("README.md"), "# knots\n").expect("readme should be writable");
    assert!(run_git(root, &["add", "README.md"]).status.success());
    assert!(run_git(root, &["commit", "-m", "init"]).status.success());
    assert!(run_git(root, &["branch", "-M", "main"]).status.success());
}

#[test]
fn hook_installer_is_idempotent() {
    let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let installer = source_root.join("scripts/repo/install-hooks.sh");
    let repo = unique_workspace("knots-hooks-idempotent");
    init_repo(&repo);

    let first = run_script(&repo, &installer, &[]);
    assert!(
        first.status.success(),
        "first install failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let managed = repo.join(".git/hooks/pre-push");
    assert!(managed.exists());
    let first_contents =
        std::fs::read_to_string(&managed).expect("managed hook should be readable");
    assert!(first_contents.contains("knots-managed-pre-push-hook"));

    let second = run_script(&repo, &installer, &[]);
    assert!(
        second.status.success(),
        "second install failed: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    let second_contents =
        std::fs::read_to_string(&managed).expect("managed hook should still be readable");
    assert_eq!(first_contents, second_contents);

    let _ = std::fs::remove_dir_all(repo);
}

#[test]
fn hook_installer_preserves_existing_local_hook() {
    let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let installer = source_root.join("scripts/repo/install-hooks.sh");
    let repo = unique_workspace("knots-hooks-preserve");
    init_repo(&repo);

    let original_hook = repo.join(".git/hooks/pre-push");
    std::fs::write(&original_hook, "#!/usr/bin/env bash\necho legacy\n")
        .expect("original hook should be writable");
    let chmod = Command::new("chmod")
        .args(["+x", original_hook.to_str().expect("utf8 path")])
        .status()
        .expect("chmod should run");
    assert!(chmod.success());

    let output = run_script(&repo, &installer, &[]);
    assert!(
        output.status.success(),
        "install failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let local_hook = repo.join(".git/hooks/pre-push.local");
    assert!(local_hook.exists());
    let local_contents = std::fs::read_to_string(&local_hook).expect("local hook should be read");
    assert!(local_contents.contains("legacy"));

    let managed = std::fs::read_to_string(repo.join(".git/hooks/pre-push"))
        .expect("managed pre-push hook should be readable");
    assert!(managed.contains("pre-push.local"));

    let _ = std::fs::remove_dir_all(repo);
}

#[test]
fn managed_hook_blocks_push_when_sanity_fails() {
    let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let installer = source_root.join("scripts/repo/install-hooks.sh");
    let root = unique_workspace("knots-hooks-block-push");
    let remote = root.join("remote.git");
    let repo = root.join("repo");

    std::fs::create_dir_all(&repo).expect("repo directory should be creatable");
    assert!(
        run_git(&root, &["init", "--bare", remote.to_str().expect("utf8")])
            .status
            .success()
    );
    init_repo(&repo);
    assert!(run_git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            remote.to_str().expect("utf8 remote path"),
        ],
    )
    .status
    .success());
    assert!(run_git(&repo, &["push", "-u", "origin", "main"])
        .status
        .success());

    let scripts_repo = repo.join("scripts/repo");
    std::fs::create_dir_all(&scripts_repo).expect("scripts/repo should be creatable");
    std::fs::write(
        scripts_repo.join("pre-push-sanity.sh"),
        "#!/usr/bin/env bash\nset -euo pipefail\necho forced-fail >&2\nexit 1\n",
    )
    .expect("failing pre-push sanity script should be writable");
    let chmod = Command::new("chmod")
        .args([
            "+x",
            scripts_repo
                .join("pre-push-sanity.sh")
                .to_str()
                .expect("utf8 path"),
        ])
        .status()
        .expect("chmod should run");
    assert!(chmod.success());

    let install = run_script(&repo, &installer, &[]);
    assert!(
        install.status.success(),
        "install failed: {}",
        String::from_utf8_lossy(&install.stderr)
    );

    std::fs::write(repo.join("CHANGE.txt"), "change\n").expect("change file should be writable");
    assert!(run_git(&repo, &["add", "CHANGE.txt"]).status.success());
    assert!(run_git(&repo, &["commit", "-m", "change"]).status.success());

    let push = run_git(&repo, &["push", "origin", "main"]);
    assert!(
        !push.status.success(),
        "push unexpectedly succeeded:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&push.stdout),
        String::from_utf8_lossy(&push.stderr)
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn threshold_regression_script_fails_on_lower_value() {
    let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let checker = source_root.join("scripts/repo/check-coverage-threshold.sh");
    let repo = unique_workspace("knots-threshold-check");
    init_repo(&repo);

    std::fs::create_dir_all(repo.join(".ci")).expect(".ci should be creatable");
    std::fs::write(repo.join(".ci/coverage-threshold.txt"), "70\n")
        .expect("threshold file should be writable");
    assert!(run_git(&repo, &["add", ".ci/coverage-threshold.txt"])
        .status
        .success());
    assert!(run_git(&repo, &["commit", "-m", "add threshold baseline"])
        .status
        .success());

    let pass = run_script(&repo, &checker, &["HEAD"]);
    assert!(
        pass.status.success(),
        "expected pass but failed: {}",
        String::from_utf8_lossy(&pass.stderr)
    );

    std::fs::write(repo.join(".ci/coverage-threshold.txt"), "65\n")
        .expect("threshold file should be writable");
    let fail = run_script(&repo, &checker, &["HEAD"]);
    assert!(
        !fail.status.success(),
        "expected failure but succeeded.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&fail.stdout),
        String::from_utf8_lossy(&fail.stderr)
    );

    let _ = std::fs::remove_dir_all(repo);
}

#[test]
fn agents_and_claude_files_enforce_same_guardrail_rules() {
    let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let agents =
        std::fs::read_to_string(source_root.join("AGENTS.md")).expect("AGENTS.md should read");
    let claude =
        std::fs::read_to_string(source_root.join("CLAUDE.md")).expect("CLAUDE.md should read");

    assert!(agents.contains("## Pre-Push Sanity (Required)"));
    assert!(agents.contains("## Coverage Ratchet Rule"));
    assert_eq!(agents, claude);
}
