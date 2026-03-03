use std::path::Path;
use std::process::Command;

use super::KNOTS_IGNORE_RULE;
use super::{ensure_knots_gitignore, uninit_local_store, warn_if_beads_hooks_present};

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

#[test]
fn warn_if_beads_hooks_present_handles_config_without_matching_hook_files() {
    let root = std::env::temp_dir().join(format!("knots-init-hooks-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    run_git(&root, &["init"]);
    run_git(&root, &["config", "user.email", "knots@example.com"]);
    run_git(&root, &["config", "user.name", "Knots Test"]);
    run_git(&root, &["config", "beads.role", "maintainer"]);
    let hooks_dir = root.join(".git/hooks");
    std::fs::create_dir_all(&hooks_dir).expect("hooks dir should be creatable");
    std::fs::write(hooks_dir.join("pre-push"), "#!/bin/sh\necho plain\n")
        .expect("non-beads pre-push should be writable");

    warn_if_beads_hooks_present(&root).expect("beads warning path should be non-fatal");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn gitignore_helpers_cover_append_and_noop_removal_paths() {
    let root = std::env::temp_dir().join(format!("knots-init-gitignore-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    let db_path = root.join(".knots/cache/state.sqlite");
    let gitignore = root.join(".gitignore");
    std::fs::write(&gitignore, "target").expect("gitignore fixture should write");

    ensure_knots_gitignore(&root).expect("ensure gitignore should succeed");
    let contents = std::fs::read_to_string(&gitignore).expect("gitignore should read");
    assert!(contents.contains("target\n"));
    assert!(contents.lines().any(|line| line == KNOTS_IGNORE_RULE));

    std::fs::write(&gitignore, "target\n").expect("gitignore reset should write");
    uninit_local_store(&root, db_path.to_str().expect("utf8 db path"))
        .expect("uninit should no-op when knots rule is absent");
    let unchanged = std::fs::read_to_string(&gitignore).expect("gitignore should read");
    assert_eq!(unchanged, "target\n");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn init_all_installs_sync_hooks() {
    use std::path::PathBuf;

    fn setup_repo_with_remote_for_hooks() -> (PathBuf, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("knots-init-hooks-int-{}", uuid::Uuid::now_v7()));
        let remote = root.join("remote.git");
        let local = root.join("local");
        std::fs::create_dir_all(&local).expect("local dir");
        run_git(&root, &["init", "--bare", remote.to_str().expect("utf8")]);
        run_git(&local, &["init"]);
        run_git(&local, &["config", "user.email", "knots@example.com"]);
        run_git(&local, &["config", "user.name", "Knots Test"]);
        std::fs::write(local.join("README.md"), "# test\n").unwrap();
        run_git(&local, &["add", "README.md"]);
        run_git(&local, &["commit", "-m", "init"]);
        run_git(
            &local,
            &["remote", "add", "origin", remote.to_str().expect("utf8")],
        );
        (root, local)
    }

    let (root, local) = setup_repo_with_remote_for_hooks();
    let db_path = local.join(".knots/cache/state.sqlite");
    super::init_all(&local, db_path.to_str().expect("utf8")).expect("init_all should succeed");

    let hooks_dir = local.join(".git").join("hooks");
    for hook_name in crate::git_hooks::MANAGED_HOOKS {
        let hook = hooks_dir.join(hook_name);
        assert!(hook.exists(), "{hook_name} hook should exist after init");
        let contents = std::fs::read_to_string(&hook).unwrap();
        assert!(
            contents.contains("knots-managed"),
            "{hook_name} should be knots-managed"
        );
    }
    let _ = std::fs::remove_dir_all(root);
}
