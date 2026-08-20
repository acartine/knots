use std::path::Path;
use std::process::Command;

use crate::doctor::DoctorStatus;
use crate::git_hooks::{
    check_hooks, cleanup_legacy_hooks, hook_template, hook_template_with_command, hooks_status,
    install_hooks, resolve_hooks_dir, uninstall_hooks, HookInstallOutcome, KNOTS_HOOK_MARKER,
};

fn unique_workspace() -> knots_test_support::TestWorkspace {
    knots_test_support::workspace("knots-git-hooks")
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

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let mut perms = std::fs::metadata(path)
        .expect("executable fixture should exist")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).expect("fixture should be executable");
}

fn setup_git_repo() -> knots_test_support::TestWorkspace {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    run_git(&root, &["init"]);
    run_git(&root, &["config", "user.email", "knots@example.com"]);
    run_git(&root, &["config", "user.name", "Knots Test"]);
    std::fs::write(root.join("README.md"), "# test\n").expect("readme should write");
    run_git(&root, &["add", "README.md"]);
    run_git(&root, &["commit", "-m", "init"]);
    root_ws
}

#[test]
fn resolve_hooks_dir_defaults_to_git_hooks() {
    let root_ws = setup_git_repo();
    let root = root_ws.path().to_path_buf();
    let dir = resolve_hooks_dir(&root);
    assert_eq!(dir, root.join(".git").join("hooks"));
}

#[test]
fn resolve_hooks_dir_respects_core_hooks_path() {
    let root_ws = setup_git_repo();
    let root = root_ws.path().to_path_buf();
    let custom = root.join("custom-hooks");
    std::fs::create_dir_all(&custom).expect("custom hooks dir");
    run_git(
        &root,
        &["config", "core.hooksPath", custom.to_str().expect("utf8")],
    );
    let dir = resolve_hooks_dir(&root);
    assert_eq!(dir, custom);
}

#[test]
fn install_hooks_creates_managed_hooks() {
    let root_ws = setup_git_repo();
    let root = root_ws.path().to_path_buf();
    let summary = install_hooks(&root).expect("install should succeed");
    assert_eq!(summary.outcomes.len(), 1);
    for (name, outcome) in &summary.outcomes {
        assert_eq!(*outcome, HookInstallOutcome::Installed);
        let path = root.join(".git").join("hooks").join(name);
        assert!(path.exists());
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains(KNOTS_HOOK_MARKER));
        assert!(contents.contains("KNO_BIN="));
        assert!(contents.contains("\"$KNO_BIN\" pull"));
        assert!(contents.contains("kno pull"));
    }
}

#[test]
fn install_hooks_is_idempotent() {
    let root_ws = setup_git_repo();
    let root = root_ws.path().to_path_buf();
    install_hooks(&root).expect("first install");
    let summary = install_hooks(&root).expect("second install");
    for (_, outcome) in &summary.outcomes {
        assert_eq!(*outcome, HookInstallOutcome::AlreadyManaged);
    }
}

#[test]
fn install_hooks_preserves_existing_to_local() {
    let root_ws = setup_git_repo();
    let root = root_ws.path().to_path_buf();
    let hooks_dir = root.join(".git").join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();
    std::fs::write(hooks_dir.join("post-merge"), "#!/bin/sh\necho user hook\n").unwrap();

    let summary = install_hooks(&root).expect("install should succeed");
    let pm = summary
        .outcomes
        .iter()
        .find(|(n, _)| n == "post-merge")
        .unwrap();
    assert_eq!(pm.1, HookInstallOutcome::PreservedExisting);

    let local = hooks_dir.join("post-merge.local");
    assert!(local.exists());
    let local_contents = std::fs::read_to_string(&local).unwrap();
    assert!(local_contents.contains("echo user hook"));

    let managed = std::fs::read_to_string(hooks_dir.join("post-merge")).unwrap();
    assert!(managed.contains(KNOTS_HOOK_MARKER));
}

#[test]
fn uninstall_hooks_removes_managed_and_restores_local() {
    let root_ws = setup_git_repo();
    let root = root_ws.path().to_path_buf();
    let hooks_dir = root.join(".git").join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();
    std::fs::write(hooks_dir.join("post-merge"), "#!/bin/sh\necho user hook\n").unwrap();

    install_hooks(&root).expect("install");
    let summary = uninstall_hooks(&root).expect("uninstall");

    let pm = summary
        .outcomes
        .iter()
        .find(|(n, _)| n == "post-merge")
        .unwrap();
    assert_eq!(pm.1, HookInstallOutcome::Installed);

    let restored = std::fs::read_to_string(hooks_dir.join("post-merge")).unwrap();
    assert!(restored.contains("echo user hook"));
    assert!(!hooks_dir.join("post-merge.local").exists());
}

#[test]
fn uninstall_hooks_noop_when_not_installed() {
    let root_ws = setup_git_repo();
    let root = root_ws.path().to_path_buf();
    let summary = uninstall_hooks(&root).expect("uninstall");
    for (_, outcome) in &summary.outcomes {
        assert_eq!(*outcome, HookInstallOutcome::AlreadyManaged);
    }
}

#[test]
fn check_hooks_warns_when_missing() {
    let root_ws = setup_git_repo();
    let root = root_ws.path().to_path_buf();
    let check = check_hooks(&root);
    assert_eq!(check.name, "hooks");
    assert_eq!(check.status, DoctorStatus::Warn);
    assert!(check.detail.contains("missing sync hooks"));
}

#[test]
fn check_hooks_passes_when_installed() {
    let root_ws = setup_git_repo();
    let root = root_ws.path().to_path_buf();
    install_hooks(&root).expect("install");
    let check = check_hooks(&root);
    assert_eq!(check.status, DoctorStatus::Pass);
    assert!(check.detail.contains("sync hooks installed"));
}

#[test]
fn check_hooks_warns_for_non_git_directory() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let check = check_hooks(&root);
    assert_eq!(check.status, DoctorStatus::Warn);
    assert!(check.detail.contains("not a git repository"));
}

#[test]
fn hooks_status_reports_installation_state() {
    let root_ws = setup_git_repo();
    let root = root_ws.path().to_path_buf();
    let before = hooks_status(&root);
    assert!(before.hooks.iter().all(|(_, managed)| !managed));

    install_hooks(&root).expect("install");
    let after = hooks_status(&root);
    assert!(after.hooks.iter().all(|(_, managed)| *managed));
}

#[test]
fn hook_template_contains_marker_and_sync() {
    let tmpl = hook_template("post-merge");
    assert!(tmpl.contains("knots-managed-post-merge-hook"));
    assert!(tmpl.contains("KNO_BIN="));
    assert!(tmpl.contains("\"$KNO_BIN\" pull"));
    assert!(tmpl.contains("command -v kno"));
    assert!(tmpl.contains("kno pull"));
    assert!(tmpl.contains("post-merge.local"));
    assert!(tmpl.starts_with("#!/usr/bin/env bash"));
}

#[cfg(unix)]
#[test]
fn hook_template_uses_installed_binary_when_path_is_stripped() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let tools_dir = root.join("tools");
    let hooks_dir = root.join("hooks");
    std::fs::create_dir_all(&tools_dir).expect("tools dir should be creatable");
    std::fs::create_dir_all(&hooks_dir).expect("hooks dir should be creatable");

    let fake_kno = tools_dir.join("kno with spaces");
    std::fs::write(
        &fake_kno,
        "#!/usr/bin/env bash\n\
         script_dir=\"$(cd \"$(dirname \"$0\")\" && pwd)\"\n\
         printf '%s\\n' \"$*\" > \"$script_dir/invoked\"\n",
    )
    .expect("fake kno should be writable");
    make_executable(&fake_kno);

    let hook_path = hooks_dir.join("post-merge");
    std::fs::write(
        &hook_path,
        hook_template_with_command("post-merge", fake_kno.to_str().expect("utf8 path")),
    )
    .expect("hook should be writable");
    make_executable(&hook_path);

    let output = Command::new(&hook_path)
        .current_dir(&root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .expect("hook should run");
    assert!(
        output.status.success(),
        "hook failed with stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let invoked = std::fs::read_to_string(tools_dir.join("invoked"))
        .expect("fake kno should record invocation");
    assert_eq!(invoked, "pull\n");
}

#[test]
fn install_preserves_existing_to_backup_when_local_exists() {
    let root_ws = setup_git_repo();
    let root = root_ws.path().to_path_buf();
    let hooks_dir = root.join(".git").join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();
    std::fs::write(hooks_dir.join("post-merge"), "#!/bin/sh\necho original\n").unwrap();
    std::fs::write(
        hooks_dir.join("post-merge.local"),
        "#!/bin/sh\necho local\n",
    )
    .unwrap();

    let summary = install_hooks(&root).expect("install");
    let pm = summary
        .outcomes
        .iter()
        .find(|(n, _)| n == "post-merge")
        .unwrap();
    assert_eq!(pm.1, HookInstallOutcome::PreservedExisting);

    let local = std::fs::read_to_string(hooks_dir.join("post-merge.local")).unwrap();
    assert!(local.contains("echo local"));

    let backups: Vec<_> = std::fs::read_dir(&hooks_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("post-merge.backup.")
        })
        .collect();
    assert_eq!(backups.len(), 1);
}

#[test]
fn check_hooks_warns_on_stale_content() {
    let root_ws = setup_git_repo();
    let root = root_ws.path().to_path_buf();
    install_hooks(&root).expect("install");
    let hooks_dir = root.join(".git").join("hooks");
    let old_template = format!(
        "#!/usr/bin/env bash\n\
         # {KNOTS_HOOK_MARKER}-post-merge-hook\n\
         kno sync >/dev/null 2>&1 &\n"
    );
    std::fs::write(hooks_dir.join("post-merge"), old_template).unwrap();

    let check = check_hooks(&root);
    assert_eq!(check.status, DoctorStatus::Warn);
    assert!(check.detail.contains("stale hook content"));
}

#[test]
fn check_hooks_warns_on_legacy_hook() {
    let root_ws = setup_git_repo();
    let root = root_ws.path().to_path_buf();
    install_hooks(&root).expect("install");
    let hooks_dir = root.join(".git").join("hooks");
    let legacy = format!(
        "#!/usr/bin/env bash\n\
         # {KNOTS_HOOK_MARKER}-post-commit-hook\n\
         kno sync >/dev/null 2>&1 &\n"
    );
    std::fs::write(hooks_dir.join("post-commit"), legacy).unwrap();

    let check = check_hooks(&root);
    assert_eq!(check.status, DoctorStatus::Warn);
    assert!(check.detail.contains("orphaned legacy hooks"));
}

#[test]
fn cleanup_legacy_hooks_removes_orphaned_hook() {
    let root_ws = setup_git_repo();
    let root = root_ws.path().to_path_buf();
    let hooks_dir = root.join(".git").join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();
    let legacy = format!(
        "#!/usr/bin/env bash\n\
         # {KNOTS_HOOK_MARKER}-post-commit-hook\n\
         kno sync >/dev/null 2>&1 &\n"
    );
    std::fs::write(hooks_dir.join("post-commit"), legacy).unwrap();

    cleanup_legacy_hooks(&root);
    assert!(!hooks_dir.join("post-commit").exists());
}

#[test]
fn cleanup_legacy_hooks_restores_local() {
    let root_ws = setup_git_repo();
    let root = root_ws.path().to_path_buf();
    let hooks_dir = root.join(".git").join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();
    let legacy = format!(
        "#!/usr/bin/env bash\n\
         # {KNOTS_HOOK_MARKER}-post-commit-hook\n\
         kno sync >/dev/null 2>&1 &\n"
    );
    std::fs::write(hooks_dir.join("post-commit"), legacy).unwrap();
    std::fs::write(
        hooks_dir.join("post-commit.local"),
        "#!/bin/sh\necho original\n",
    )
    .unwrap();

    cleanup_legacy_hooks(&root);
    let restored = std::fs::read_to_string(hooks_dir.join("post-commit")).unwrap();
    assert!(restored.contains("echo original"));
    assert!(!hooks_dir.join("post-commit.local").exists());
}
