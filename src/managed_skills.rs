use std::fmt;
use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use crate::app::AppError;
use crate::doctor::{DoctorCheck, DoctorStatus};

#[path = "managed_skills_inventory.rs"]
mod inventory;
use inventory::managed_skills;
#[path = "managed_skills_output.rs"]
mod output;
use output::{format_changed_paths, format_existing_skills, format_skill_detail, skill_paths};
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
        self.location_candidates(repo_root, home)
            .into_iter()
            .filter(|location| location.tool_root.exists())
            .collect()
    }

    fn location_candidates(self, repo_root: &Path, home: Option<&Path>) -> Vec<SkillLocation> {
        let mut locations = Vec::new();
        match self {
            SkillTool::Codex => {
                if let Some(home) = home {
                    push_location(
                        &mut locations,
                        LocationScope::User,
                        home.join(".codex"),
                        "skills",
                    );
                }
            }
            SkillTool::Claude => {
                push_location(
                    &mut locations,
                    LocationScope::Project,
                    repo_root.join(".claude"),
                    "skills",
                );
                if let Some(home) = home {
                    push_location(
                        &mut locations,
                        LocationScope::User,
                        home.join(".claude"),
                        "skills",
                    );
                }
            }
            SkillTool::OpenCode => {
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
        }
        locations
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

pub fn fix_doctor_check(repo_root: &Path, check_name: &str) {
    let Some(tool) = tool_for_check_name(check_name) else {
        return;
    };
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let Ok(destination) = preferred_location(repo_root, home.as_deref(), tool) else {
        return;
    };
    let _ = reconcile_skills(&destination);
}

fn doctor_checks_with_home(repo_root: &Path, home: Option<&Path>) -> Vec<DoctorCheck> {
    [SkillTool::Codex, SkillTool::Claude, SkillTool::OpenCode]
        .into_iter()
        .map(|tool| doctor_check(repo_root, home, tool))
        .collect()
}

fn doctor_check(repo_root: &Path, home: Option<&Path>, tool: SkillTool) -> DoctorCheck {
    let preferred = doctor_location(repo_root, home, tool);
    let (status, detail) = match preferred {
        Some(location) => {
            let state = inspect_location(&location);
            if state.is_current() {
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
                    format_skill_detail(tool, &location, &state.missing, &state.drifted),
                )
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

fn install_missing(
    repo_root: &Path,
    home: Option<&Path>,
    tool: SkillTool,
) -> Result<String, AppError> {
    let destination = preferred_location(repo_root, home, tool)?;
    let missing = inspect_location(&destination).missing;
    if missing.is_empty() {
        return Ok(format_existing_skills(tool, &destination));
    }
    let changed = write_skills(&destination, &missing)?;
    Ok(format_changed_paths(tool, "installed", &changed))
}

fn uninstall_managed(
    repo_root: &Path,
    home: Option<&Path>,
    tool: SkillTool,
) -> Result<String, AppError> {
    let locations = tool.locations(repo_root, home);
    let mut removed_paths = Vec::new();
    for location in locations {
        let installed = installed_skills(&location);
        if installed.is_empty() {
            continue;
        }
        removed_paths.extend(remove_skills(&location, &installed)?);
    }
    if removed_paths.is_empty() {
        return Err(AppError::InvalidArgument(format!(
            "{} has no installed managed skills to uninstall",
            tool.display_name()
        )));
    }
    Ok(format_changed_paths(tool, "removed", &removed_paths))
}

fn update_managed(
    repo_root: &Path,
    home: Option<&Path>,
    interactive: bool,
    tool: SkillTool,
) -> Result<String, AppError> {
    let destination = preferred_location(repo_root, home, tool)?;
    let missing = inspect_location(&destination).missing;
    if !missing.is_empty() {
        if !interactive {
            return Err(AppError::InvalidArgument(format!(
                "{} missing managed skills at {}; run `kno skills install {}` or rerun \
                 interactively",
                tool.display_name(),
                destination.skills_root.display(),
                tool.slug()
            )));
        }
        let mut stderr = io::stderr();
        let mut stdin = io::stdin().lock();
        if !prompt_install_missing(&mut stderr, &mut stdin, tool, &destination, &missing)? {
            return Err(AppError::InvalidArgument(
                "managed skill update cancelled; no changes written".to_string(),
            ));
        }
        write_skills(&destination, &missing)?;
    }

    let installed_locations = installed_locations(repo_root, home, tool);
    let mut updated_paths = Vec::new();
    for location in &installed_locations {
        let installed = installed_skills(location);
        updated_paths.extend(write_skills(location, &installed)?);
    }
    Ok(format_changed_paths(tool, "updated", &updated_paths))
}

fn prompt_install_missing<W: Write, R: BufRead>(
    writer: &mut W,
    reader: &mut R,
    tool: SkillTool,
    destination: &SkillLocation,
    missing: &[ManagedSkill],
) -> Result<bool, AppError> {
    writeln!(
        writer,
        "{} is missing managed skills at {}:",
        tool.display_name(),
        destination.skills_root.display()
    )?;
    for path in skill_paths(destination, missing) {
        writeln!(writer, "  {}", path.display())?;
    }
    write!(writer, "install missing skills before update? [y/N]: ")?;
    writer.flush()?;

    let mut input = String::new();
    reader.read_line(&mut input)?;
    Ok(matches!(
        input.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn preferred_location(
    repo_root: &Path,
    home: Option<&Path>,
    tool: SkillTool,
) -> Result<SkillLocation, AppError> {
    doctor_location(repo_root, home, tool).ok_or_else(|| {
        AppError::InvalidArgument(format!(
            "{} root not detected; create {} first",
            tool.display_name(),
            expected_root_hint(tool)
        ))
    })
}

fn installed_locations(
    repo_root: &Path,
    home: Option<&Path>,
    tool: SkillTool,
) -> Vec<SkillLocation> {
    tool.locations(repo_root, home)
        .into_iter()
        .filter(|location| !installed_skills(location).is_empty())
        .collect()
}

fn installed_skills(location: &SkillLocation) -> Vec<ManagedSkill> {
    managed_skills()
        .iter()
        .copied()
        .filter(|skill| skill_path(location, *skill).exists())
        .collect()
}

fn write_skills(
    location: &SkillLocation,
    skills: &[ManagedSkill],
) -> Result<Vec<PathBuf>, AppError> {
    if skills.is_empty() {
        return Ok(Vec::new());
    }
    fs::create_dir_all(&location.skills_root)?;
    let mut written = Vec::with_capacity(skills.len());
    for skill in skills {
        let path = skill_path(location, *skill);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, render_skill(*skill))?;
        written.push(path);
    }
    Ok(written)
}

fn remove_skills(
    location: &SkillLocation,
    skills: &[ManagedSkill],
) -> Result<Vec<PathBuf>, AppError> {
    let mut removed = Vec::new();
    for skill in skills {
        let path = skill_path(location, *skill);
        if path.exists() {
            fs::remove_file(&path)?;
            removed.push(path.clone());
        }
        if let Some(dir) = path.parent() {
            remove_dir_if_empty(dir)?;
        }
    }
    remove_dir_if_empty(&location.skills_root)?;
    Ok(removed)
}

fn remove_dir_if_empty(path: &Path) -> Result<(), AppError> {
    let Ok(mut entries) = fs::read_dir(path) else {
        return Ok(());
    };
    if entries.next().is_none() {
        fs::remove_dir(path)?;
    }
    Ok(())
}

fn render_skill(skill: ManagedSkill) -> String {
    skill.contents.to_string()
}

fn skill_path(location: &SkillLocation, skill: ManagedSkill) -> PathBuf {
    location
        .skills_root
        .join(skill.deploy_name)
        .join("SKILL.md")
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
        .or_else(|| candidates.into_iter().next())
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
        SkillTool::Codex => "~/.codex",
        SkillTool::Claude => ".claude or ~/.claude",
        SkillTool::OpenCode => ".opencode or ~/.config/opencode",
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
