use std::path::PathBuf;

use super::{installed_skills, skill_path, ManagedSkill, SkillLocation, SkillTool};

pub(super) fn skill_paths(location: &SkillLocation, skills: &[ManagedSkill]) -> Vec<PathBuf> {
    skills
        .iter()
        .map(|skill| skill_path(location, *skill))
        .collect()
}

pub(super) fn format_existing_skills(tool: SkillTool, location: &SkillLocation) -> String {
    format_changed_paths(
        tool,
        "already installed",
        &skill_paths(location, &installed_skills(location)),
    )
}

pub(super) fn format_changed_paths(tool: SkillTool, verb: &str, paths: &[PathBuf]) -> String {
    let mut output = format!(
        "{} {} {} managed skill(s):",
        tool.display_name(),
        verb,
        paths.len()
    );
    for path in paths {
        output.push('\n');
        output.push_str("- ");
        output.push_str(&path.display().to_string());
    }
    output
}

pub(super) fn format_missing_detail(
    tool: SkillTool,
    location: &SkillLocation,
    missing: &[ManagedSkill],
) -> String {
    let paths = skill_paths(location, missing);
    format!(
        "{} missing managed skills at {}: {}; run `kno skills install {}`",
        tool.display_name(),
        location.skills_root.display(),
        display_paths(&paths),
        tool.slug()
    )
}

fn display_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}
