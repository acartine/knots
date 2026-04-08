use std::path::Path;
use std::process::Command;

use super::KNOTS_IGNORE_RULE;
use super::{
    ensure_knots_gitignore, init_local_store, uninit_local_store, warn_if_beads_hooks_present,
};
use crate::project::{create_named_project, resolve_context, DistributionMode};

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

#[test]
fn run_git_panics_with_stderr_when_command_fails() {
    let root = std::env::temp_dir().join(format!("knots-init-git-panic-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    let panic = std::panic::catch_unwind(|| run_git(&root, &["status"]));
    assert!(panic.is_err(), "run_git should panic for non-repo paths");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn init_local_store_for_named_local_only_project_skips_repo_artifacts() {
    let home = std::env::temp_dir().join(format!("knots-init-local-only-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&home).expect("home should be creatable");
    create_named_project(Some(&home), "demo", None).expect("named project should be created");

    let context = resolve_context(Some("demo"), None, &home, Some(&home))
        .expect("named project context should resolve");
    assert_eq!(context.distribution, DistributionMode::LocalOnly);
    assert_eq!(context.repo_root, context.store_paths.root);

    init_local_store(
        &context.repo_root,
        context
            .store_paths
            .db_path()
            .to_str()
            .expect("utf8 db path"),
    )
    .expect("local-only init should succeed");

    assert!(context.store_paths.db_path().exists());
    assert!(!context.repo_root.join(".gitignore").exists());
    let workflows_root = crate::installed_workflows::workflows_root(&context.repo_root);
    assert!(workflows_root.join("current").exists());

    let registry = crate::installed_workflows::InstalledWorkflowRegistry::load(&context.repo_root)
        .expect("workflow registry should load");
    assert_eq!(
        registry.current_workflow_id_for_knot_type(crate::domain::knot_type::KnotType::Work),
        "work_sdlc"
    );
    assert_eq!(
        registry.current_workflow_id_for_knot_type(crate::domain::knot_type::KnotType::Explore),
        "explore_sdlc"
    );

    let _ = std::fs::remove_dir_all(home);
}
