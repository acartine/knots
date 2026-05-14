use std::fmt;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};

use crate::app::AppError;
use crate::doctor::{DoctorCheck, DoctorStatus};

#[path = "managed_skills_inventory.rs"]
mod inventory;
use inventory::managed_skills;
#[path = "managed_skills_gitignore.rs"]
mod gitignore;
use gitignore::{
    ensure_agents_skills_gitignore, ensure_claude_skills_gitignore, has_agents_skills_gitignore,
    has_claude_skills_gitignore,
};
#[path = "managed_skills_ops.rs"]
mod ops;
use ops::{
    cleanup_legacy_locations, install_missing, installed_skills, render_skill, skill_path,
    uninstall_managed, update_managed, write_skills,
};
#[cfg(test)]
use ops::{installed_locations, preferred_location, prompt_install_missing, remove_dir_if_empty};
#[path = "managed_skills_output.rs"]
mod output;
use output::format_skill_detail;
#[path = "managed_skills_state.rs"]
mod state;
use state::{inspect_location, reconcile_skills};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillTool {
    Codex,
    Claude,
    OpenCode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocationScope {
    Project,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SkillLocation {
    scope: LocationScope,
    tool_root: PathBuf,
    skills_root: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ManagedSkill {
    deploy_name: &'static str,
    contents: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillsCommand {
    Install(SkillTool),
    Uninstall(SkillTool),
    Update(SkillTool),
}

impl SkillTool {
    pub fn slug(self) -> &'static str {
        match self {
            SkillTool::Codex => "codex",
            SkillTool::Claude => "claude",
            SkillTool::OpenCode => "opencode",
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            SkillTool::Codex => "Codex",
            SkillTool::Claude => "Claude",
            SkillTool::OpenCode => "OpenCode",
        }
    }

    fn doctor_check_name(self) -> String {
        format!("skills_{}", self.slug())
    }

    fn locations(self, repo_root: &Path, home: Option<&Path>) -> Vec<SkillLocation> {
        let mut locations = self
            .location_candidates(repo_root, home)
            .into_iter()
            .filter(|location| location.tool_root.exists())
            .collect::<Vec<_>>();
        for legacy in self.legacy_cleanup_locations(repo_root, home) {
            if !locations
                .iter()
                .any(|location| location.skills_root == legacy.skills_root)
            {
                locations.push(legacy);
            }
        }
        locations
    }

    fn requires_existing_root(self) -> bool {
        matches!(self, SkillTool::Claude)
    }

    fn uses_agents_root(self) -> bool {
        matches!(self, SkillTool::Codex | SkillTool::OpenCode)
    }

    fn location_candidates(self, repo_root: &Path, _home: Option<&Path>) -> Vec<SkillLocation> {
        let mut locations = Vec::new();
        match self {
            SkillTool::Codex | SkillTool::OpenCode => {
                push_location(
                    &mut locations,
                    LocationScope::Project,
                    repo_root.join(".agents"),
                    "skills",
                );
            }
            SkillTool::Claude => {
                push_location(
                    &mut locations,
                    LocationScope::Project,
                    repo_root.join(".claude"),
                    "skills",
                );
            }
        }
        locations
    }

    fn legacy_cleanup_locations(self, repo_root: &Path, home: Option<&Path>) -> Vec<SkillLocation> {
        let mut locations = Vec::new();
        if self == SkillTool::OpenCode {
            push_location(
                &mut locations,
                LocationScope::Project,
                repo_root.join(".opencode"),
                "skills",
            );
            if let Some(home) = home {
                push_location(
                    &mut locations,
                    LocationScope::User,
                    home.join(".config").join("opencode"),
                    "skills",
                );
            }
        }
        locations
            .into_iter()
            .filter(|location| location.tool_root.exists())
            .collect()
    }
}

impl fmt::Display for SkillTool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

pub fn run_command(repo_root: &Path, command: SkillsCommand) -> Result<String, AppError> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let interactive = io::stdin().is_terminal();
    run_command_with_io(repo_root, home.as_deref(), interactive, command)
}

pub fn doctor_checks(repo_root: &Path) -> Vec<DoctorCheck> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    doctor_checks_with_home(repo_root, home.as_deref())
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn fix_doctor_check(repo_root: &Path, check_name: &str) {
    let _ = try_fix_doctor_check(repo_root, check_name);
}

pub(crate) fn try_fix_doctor_check(repo_root: &Path, check_name: &str) -> Result<bool, AppError> {
    let Some(tool) = tool_for_check_name(check_name) else {
        return Ok(false);
    };
    let home = std::env::var_os("HOME").map(PathBuf::from);
    cleanup_legacy_locations(repo_root, home.as_deref(), tool)?;
    if tool.uses_agents_root() && !repo_root.join(".agents").exists() {
        return Ok(false);
    }
    let Some(destination) = doctor_location(repo_root, home.as_deref(), tool) else {
        return Ok(false);
    };
    match tool {
        SkillTool::Codex | SkillTool::OpenCode => {
            ensure_agents_skills_gitignore(repo_root)?;
        }
        SkillTool::Claude => {
            ensure_claude_skills_gitignore(repo_root)?;
        }
    }
    reconcile_skills(&destination)?;
    Ok(true)
}

fn doctor_checks_with_home(repo_root: &Path, home: Option<&Path>) -> Vec<DoctorCheck> {
    [SkillTool::Codex, SkillTool::Claude, SkillTool::OpenCode]
        .into_iter()
        .map(|tool| doctor_check(repo_root, home, tool))
        .collect()
}

fn doctor_check(repo_root: &Path, home: Option<&Path>, tool: SkillTool) -> DoctorCheck {
    if tool.uses_agents_root() && !repo_root.join(".agents").exists() {
        return DoctorCheck::simple(
            tool.doctor_check_name(),
            DoctorStatus::Pass,
            format!(
                "{} managed skills not configured; .agents root absent",
                tool.display_name()
            ),
        );
    }
    let preferred = doctor_location(repo_root, home, tool);
    let (status, detail) = match preferred {
        Some(location) => {
            let state = inspect_location(&location);
            let gitignore_ok = managed_skills_gitignore_ok(repo_root, tool).unwrap_or(false);
            if state.is_current() {
                if gitignore_ok {
                    (
                        DoctorStatus::Pass,
                        format!(
                            "{} managed skills installed at {}",
                            tool.display_name(),
                            location.skills_root.display()
                        ),
                    )
                } else {
                    (
                        DoctorStatus::Warn,
                        format!(
                            "{} managed skills installed at {}, but .gitignore does not blocklist \
                             {} except skills; run `kno doctor --fix`",
                            tool.display_name(),
                            location.skills_root.display(),
                            managed_root_hint(tool)
                        ),
                    )
                }
            } else {
                let mut detail =
                    format_skill_detail(tool, &location, &state.missing, &state.drifted);
                if !gitignore_ok {
                    detail.push_str(&format!(
                        "; .gitignore does not blocklist {} except skills; run `kno doctor --fix`",
                        managed_root_hint(tool)
                    ));
                }
                (DoctorStatus::Warn, detail)
            }
        }
        None => (
            DoctorStatus::Warn,
            format!(
                "{} root not detected; create {} and run `kno skills install {}`",
                tool.display_name(),
                expected_root_hint(tool),
                tool.slug()
            ),
        ),
    };

    DoctorCheck {
        name: tool.doctor_check_name(),
        status,
        detail,
        data: None,
    }
}

fn managed_skills_gitignore_ok(repo_root: &Path, tool: SkillTool) -> Result<bool, AppError> {
    match tool {
        SkillTool::Codex | SkillTool::OpenCode => has_agents_skills_gitignore(repo_root),
        SkillTool::Claude => has_claude_skills_gitignore(repo_root),
    }
}

fn managed_root_hint(tool: SkillTool) -> &'static str {
    match tool {
        SkillTool::Codex | SkillTool::OpenCode => ".agents",
        SkillTool::Claude => ".claude",
    }
}

fn run_command_with_io(
    repo_root: &Path,
    home: Option<&Path>,
    interactive: bool,
    command: SkillsCommand,
) -> Result<String, AppError> {
    match command {
        SkillsCommand::Install(tool) => install_missing(repo_root, home, tool),
        SkillsCommand::Uninstall(tool) => uninstall_managed(repo_root, home, tool),
        SkillsCommand::Update(tool) => update_managed(repo_root, home, interactive, tool),
    }
}

fn doctor_location(
    repo_root: &Path,
    home: Option<&Path>,
    tool: SkillTool,
) -> Option<SkillLocation> {
    let candidates = tool.location_candidates(repo_root, home);
    candidates
        .iter()
        .find(|location| location.tool_root.exists())
        .cloned()
        .or_else(|| {
            candidates
                .iter()
                .find(|location| location.scope == LocationScope::User)
                .cloned()
        })
        .or_else(|| {
            (!tool.requires_existing_root())
                .then(|| candidates.into_iter().next())
                .flatten()
        })
}

fn push_location(
    locations: &mut Vec<SkillLocation>,
    scope: LocationScope,
    tool_root: PathBuf,
    skills_dir_name: &str,
) {
    locations.push(SkillLocation {
        scope,
        skills_root: tool_root.join(skills_dir_name),
        tool_root,
    });
}

fn expected_root_hint(tool: SkillTool) -> &'static str {
    match tool {
        SkillTool::Codex => ".agents",
        SkillTool::Claude => "./.claude",
        SkillTool::OpenCode => ".agents",
    }
}

fn tool_for_check_name(name: &str) -> Option<SkillTool> {
    match name {
        "skills_codex" => Some(SkillTool::Codex),
        "skills_claude" => Some(SkillTool::Claude),
        "skills_opencode" => Some(SkillTool::OpenCode),
        _ => None,
    }
}

#[cfg(test)]
#[path = "managed_skills_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "managed_skills_tests_ext.rs"]
mod tests_ext;
