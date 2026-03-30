use std::path::{Path, PathBuf};
use std::process::Command;

use crate::doctor::{DoctorCheck, DoctorStatus};

pub const MANAGED_HOOKS: &[&str] = &["post-merge"];
pub(crate) const KNOTS_HOOK_MARKER: &str = "knots-managed";
const LEGACY_HOOKS: &[&str] = &["post-commit"];

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

fn is_hook_current(path: &Path, hook_name: &str) -> bool {
    if let Ok(contents) = std::fs::read_to_string(path) {
        contents == hook_template(hook_name)
    } else {
        false
    }
}

pub(crate) fn hook_template(hook_name: &str) -> String {
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
    let mut problems: Vec<String> = Vec::new();

    let missing: Vec<&str> = MANAGED_HOOKS
        .iter()
        .filter(|h| !is_knots_managed(&hooks_dir.join(h)))
        .copied()
        .collect();
    if !missing.is_empty() {
        problems.push(format!("missing sync hooks: {}", missing.join(", ")));
    }

    let stale: Vec<&str> = MANAGED_HOOKS
        .iter()
        .filter(|h| {
            let p = hooks_dir.join(h);
            is_knots_managed(&p) && !is_hook_current(&p, h)
        })
        .copied()
        .collect();
    if !stale.is_empty() {
        problems.push(format!("stale hook content: {}", stale.join(", ")));
    }

    let legacy: Vec<&str> = LEGACY_HOOKS
        .iter()
        .filter(|h| is_knots_managed(&hooks_dir.join(h)))
        .copied()
        .collect();
    if !legacy.is_empty() {
        problems.push(format!("orphaned legacy hooks: {}", legacy.join(", ")));
    }

    if problems.is_empty() {
        DoctorCheck {
            name: "hooks".to_string(),
            status: DoctorStatus::Pass,
            detail: "sync hooks installed".to_string(),
        }
    } else {
        DoctorCheck {
            name: "hooks".to_string(),
            status: DoctorStatus::Warn,
            detail: format!("{} (run `kno doctor --fix`)", problems.join("; ")),
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

pub fn cleanup_legacy_hooks(repo_root: &Path) {
    let hooks_dir = resolve_hooks_dir(repo_root);
    for hook_name in LEGACY_HOOKS {
        let _ = uninstall_hook(&hooks_dir, hook_name);
    }
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
