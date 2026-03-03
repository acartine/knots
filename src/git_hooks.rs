use std::path::{Path, PathBuf};
use std::process::Command;

use crate::doctor::{DoctorCheck, DoctorStatus};

pub const MANAGED_HOOKS: &[&str] = &["post-merge"];
const KNOTS_HOOK_MARKER: &str = "knots-managed";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookInstallOutcome {
    Installed,
    AlreadyManaged,
    PreservedExisting,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HooksSummary {
    pub outcomes: Vec<(String, HookInstallOutcome)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HooksStatusReport {
    pub hooks: Vec<(String, bool)>,
}

pub fn resolve_hooks_dir(repo_root: &Path) -> PathBuf {
    if let Ok(output) = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["config", "--local", "--get", "core.hooksPath"])
        .output()
    {
        if output.status.success() {
            let configured = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !configured.is_empty() {
                let path = Path::new(&configured);
                if path.is_absolute() {
                    return path.to_path_buf();
                }
                return repo_root.join(configured);
            }
        }
    }
    repo_root.join(".git").join("hooks")
}

fn is_knots_managed(path: &Path) -> bool {
    if let Ok(contents) = std::fs::read_to_string(path) {
        contents.contains(KNOTS_HOOK_MARKER)
    } else {
        false
    }
}

fn hook_template(hook_name: &str) -> String {
    format!(
        "#!/usr/bin/env bash\n\
         # {KNOTS_HOOK_MARKER}-{hook_name}-hook\n\
         if [ -x \"$(dirname \"$0\")/{hook_name}.local\" ]; then\n\
         \x20 \"$(dirname \"$0\")/{hook_name}.local\" \"$@\"\n\
         fi\n\
         kno pull\n"
    )
}

fn install_hook(hooks_dir: &Path, hook_name: &str) -> std::io::Result<HookInstallOutcome> {
    std::fs::create_dir_all(hooks_dir)?;
    let hook_path = hooks_dir.join(hook_name);
    let local_path = hooks_dir.join(format!("{hook_name}.local"));

    if hook_path.exists() && is_knots_managed(&hook_path) {
        std::fs::write(&hook_path, hook_template(hook_name))?;
        set_executable(&hook_path)?;
        return Ok(HookInstallOutcome::AlreadyManaged);
    }

    let mut outcome = HookInstallOutcome::Installed;
    if hook_path.exists() {
        if !local_path.exists() {
            std::fs::rename(&hook_path, &local_path)?;
        } else {
            let backup = hooks_dir.join(format!(
                "{hook_name}.backup.{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            ));
            std::fs::rename(&hook_path, &backup)?;
        }
        outcome = HookInstallOutcome::PreservedExisting;
    }

    std::fs::write(&hook_path, hook_template(hook_name))?;
    set_executable(&hook_path)?;
    Ok(outcome)
}

fn uninstall_hook(hooks_dir: &Path, hook_name: &str) -> std::io::Result<bool> {
    let hook_path = hooks_dir.join(hook_name);
    if !hook_path.exists() || !is_knots_managed(&hook_path) {
        return Ok(false);
    }
    std::fs::remove_file(&hook_path)?;
    let local_path = hooks_dir.join(format!("{hook_name}.local"));
    if local_path.exists() {
        std::fs::rename(&local_path, &hook_path)?;
    }
    Ok(true)
}

pub fn check_hooks(repo_root: &Path) -> DoctorCheck {
    if !repo_root.join(".git").exists() {
        return DoctorCheck {
            name: "hooks".to_string(),
            status: DoctorStatus::Warn,
            detail: "not a git repository; skipping hook check".to_string(),
        };
    }
    let hooks_dir = resolve_hooks_dir(repo_root);
    let missing: Vec<&str> = MANAGED_HOOKS
        .iter()
        .filter(|h| !is_knots_managed(&hooks_dir.join(h)))
        .copied()
        .collect();

    if missing.is_empty() {
        DoctorCheck {
            name: "hooks".to_string(),
            status: DoctorStatus::Pass,
            detail: "sync hooks installed".to_string(),
        }
    } else {
        DoctorCheck {
            name: "hooks".to_string(),
            status: DoctorStatus::Warn,
            detail: format!(
                "missing sync hooks: {} (run `kno hooks install`)",
                missing.join(", ")
            ),
        }
    }
}

pub fn install_hooks(repo_root: &Path) -> std::io::Result<HooksSummary> {
    let hooks_dir = resolve_hooks_dir(repo_root);
    let mut outcomes = Vec::new();
    for hook_name in MANAGED_HOOKS {
        let outcome = install_hook(&hooks_dir, hook_name)?;
        outcomes.push((hook_name.to_string(), outcome));
    }
    Ok(HooksSummary { outcomes })
}

pub fn uninstall_hooks(repo_root: &Path) -> std::io::Result<HooksSummary> {
    let hooks_dir = resolve_hooks_dir(repo_root);
    let mut outcomes = Vec::new();
    for hook_name in MANAGED_HOOKS {
        let removed = uninstall_hook(&hooks_dir, hook_name)?;
        let outcome = if removed {
            HookInstallOutcome::Installed
        } else {
            HookInstallOutcome::AlreadyManaged
        };
        outcomes.push((hook_name.to_string(), outcome));
    }
    Ok(HooksSummary { outcomes })
}

pub fn hooks_status(repo_root: &Path) -> HooksStatusReport {
    let hooks_dir = resolve_hooks_dir(repo_root);
    let hooks = MANAGED_HOOKS
        .iter()
        .map(|h| (h.to_string(), is_knots_managed(&hooks_dir.join(h))))
        .collect();
    HooksStatusReport { hooks }
}

#[cfg(unix)]
fn set_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use uuid::Uuid;

    use super::*;

    fn unique_workspace() -> PathBuf {
        let root = std::env::temp_dir().join(format!("knots-git-hooks-{}", Uuid::now_v7()));
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

    fn setup_git_repo() -> PathBuf {
        let root = unique_workspace();
        run_git(&root, &["init"]);
        run_git(&root, &["config", "user.email", "knots@example.com"]);
        run_git(&root, &["config", "user.name", "Knots Test"]);
        std::fs::write(root.join("README.md"), "# test\n").expect("readme should write");
        run_git(&root, &["add", "README.md"]);
        run_git(&root, &["commit", "-m", "init"]);
        root
    }

    #[test]
    fn resolve_hooks_dir_defaults_to_git_hooks() {
        let root = setup_git_repo();
        let dir = resolve_hooks_dir(&root);
        assert_eq!(dir, root.join(".git").join("hooks"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resolve_hooks_dir_respects_core_hooks_path() {
        let root = setup_git_repo();
        let custom = root.join("custom-hooks");
        std::fs::create_dir_all(&custom).expect("custom hooks dir");
        run_git(
            &root,
            &["config", "core.hooksPath", custom.to_str().expect("utf8")],
        );
        let dir = resolve_hooks_dir(&root);
        assert_eq!(dir, custom);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn install_hooks_creates_managed_hooks() {
        let root = setup_git_repo();
        let summary = install_hooks(&root).expect("install should succeed");
        assert_eq!(summary.outcomes.len(), 1);
        for (name, outcome) in &summary.outcomes {
            assert_eq!(*outcome, HookInstallOutcome::Installed);
            let path = root.join(".git").join("hooks").join(name);
            assert!(path.exists());
            let contents = std::fs::read_to_string(&path).unwrap();
            assert!(contents.contains(KNOTS_HOOK_MARKER));
            assert!(contents.contains("kno pull"));
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn install_hooks_is_idempotent() {
        let root = setup_git_repo();
        install_hooks(&root).expect("first install");
        let summary = install_hooks(&root).expect("second install");
        for (_, outcome) in &summary.outcomes {
            assert_eq!(*outcome, HookInstallOutcome::AlreadyManaged);
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn install_hooks_preserves_existing_to_local() {
        let root = setup_git_repo();
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
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn uninstall_hooks_removes_managed_and_restores_local() {
        let root = setup_git_repo();
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
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn uninstall_hooks_noop_when_not_installed() {
        let root = setup_git_repo();
        let summary = uninstall_hooks(&root).expect("uninstall");
        for (_, outcome) in &summary.outcomes {
            assert_eq!(*outcome, HookInstallOutcome::AlreadyManaged);
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn check_hooks_warns_when_missing() {
        let root = setup_git_repo();
        let check = check_hooks(&root);
        assert_eq!(check.name, "hooks");
        assert_eq!(check.status, DoctorStatus::Warn);
        assert!(check.detail.contains("missing sync hooks"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn check_hooks_passes_when_installed() {
        let root = setup_git_repo();
        install_hooks(&root).expect("install");
        let check = check_hooks(&root);
        assert_eq!(check.status, DoctorStatus::Pass);
        assert!(check.detail.contains("sync hooks installed"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn check_hooks_warns_for_non_git_directory() {
        let root = unique_workspace();
        let check = check_hooks(&root);
        assert_eq!(check.status, DoctorStatus::Warn);
        assert!(check.detail.contains("not a git repository"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn hooks_status_reports_installation_state() {
        let root = setup_git_repo();
        let before = hooks_status(&root);
        assert!(before.hooks.iter().all(|(_, managed)| !managed));

        install_hooks(&root).expect("install");
        let after = hooks_status(&root);
        assert!(after.hooks.iter().all(|(_, managed)| *managed));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn hook_template_contains_marker_and_sync() {
        let tmpl = hook_template("post-merge");
        assert!(tmpl.contains("knots-managed-post-merge-hook"));
        assert!(tmpl.contains("kno pull"));
        assert!(tmpl.contains("post-merge.local"));
        assert!(tmpl.starts_with("#!/usr/bin/env bash"));
    }

    #[test]
    fn install_preserves_existing_to_backup_when_local_exists() {
        let root = setup_git_repo();
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
        let _ = std::fs::remove_dir_all(root);
    }
}
