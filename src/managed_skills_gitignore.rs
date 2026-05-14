use std::fs;
use std::path::Path;
use std::process::Command;

use crate::app::AppError;

const AGENTS_DIR: &str = ".agents";
const CLAUDE_DIR: &str = ".claude";

pub(super) fn ensure_agents_skills_gitignore(repo_root: &Path) -> Result<(), AppError> {
    ensure_managed_skills_gitignore(repo_root, AGENTS_DIR)
}

pub(super) fn ensure_claude_skills_gitignore(repo_root: &Path) -> Result<(), AppError> {
    ensure_managed_skills_gitignore(repo_root, CLAUDE_DIR)
}

pub(super) fn has_agents_skills_gitignore(repo_root: &Path) -> Result<bool, AppError> {
    has_managed_skills_gitignore(repo_root, AGENTS_DIR)
}

pub(super) fn has_claude_skills_gitignore(repo_root: &Path) -> Result<bool, AppError> {
    has_managed_skills_gitignore(repo_root, CLAUDE_DIR)
}

fn ensure_managed_skills_gitignore(repo_root: &Path, dir: &str) -> Result<(), AppError> {
    let path = repo_root.join(".gitignore");
    let contents = if path.exists() {
        fs::read_to_string(&path)?
    } else {
        String::new()
    };

    if has_managed_skills_gitignore(repo_root, dir)? {
        return Ok(());
    }

    let rules = managed_skills_rules(dir);
    let known_rules = managed_skills_rule_variants(dir);
    let mut lines = contents
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !known_rules.iter().any(|rule| rule == trimmed)
        })
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    lines.extend(rules);

    let normalized = format!("{}\n", lines.join("\n"));
    if normalized != contents {
        fs::write(path, normalized)?;
    }
    Ok(())
}

fn has_managed_skills_gitignore(repo_root: &Path, dir: &str) -> Result<bool, AppError> {
    let path = repo_root.join(".gitignore");
    if !path.exists() {
        return Ok(false);
    }
    let contents = fs::read_to_string(path)?;
    if contents.trim().is_empty() {
        return Ok(false);
    }
    if let Some(policy_matches) = git_reports_managed_skills_policy(repo_root, dir) {
        return Ok(policy_matches);
    }
    Ok(has_canonical_managed_skills_rules(&contents, dir))
}

fn managed_skills_rules(dir: &str) -> Vec<String> {
    vec![
        format!("/{dir}/**"),
        format!("!/{dir}/"),
        format!("!/{dir}/skills/"),
        format!("!/{dir}/skills/**"),
    ]
}

fn has_canonical_managed_skills_rules(contents: &str, dir: &str) -> bool {
    let lines = contents
        .lines()
        .map(str::trim)
        .collect::<std::collections::HashSet<_>>();
    managed_skills_rules(dir)
        .iter()
        .all(|rule| lines.contains(rule.as_str()))
}

fn managed_skills_rule_variants(dir: &str) -> Vec<String> {
    [
        format!("{dir}/"),
        format!("/{dir}/"),
        format!("{dir}/*"),
        format!("/{dir}/*"),
        format!("{dir}/**"),
        format!("/{dir}/**"),
        format!("!{dir}/"),
        format!("!/{dir}/"),
        format!("!{dir}/skills/"),
        format!("!/{dir}/skills/"),
        format!("!{dir}/skills/**"),
        format!("!/{dir}/skills/**"),
    ]
    .into()
}

fn git_reports_managed_skills_policy(repo_root: &Path, dir: &str) -> Option<bool> {
    let private_file = format!("{dir}/private.txt");
    let nested_worktree = format!("{dir}/worktrees/elastic-clarke-9c0ed3/.git");
    let skill_file = format!("{dir}/skills/knots/SKILL.md");

    let private = git_ignore_decision(repo_root, &private_file)?;
    let worktree = git_ignore_decision(repo_root, &nested_worktree)?;
    let skill = git_ignore_decision(repo_root, &skill_file)?;

    Some(
        private == IgnoreDecision::Ignored
            && worktree == IgnoreDecision::Ignored
            && skill == IgnoreDecision::Allowed,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IgnoreDecision {
    Ignored,
    Allowed,
}

fn git_ignore_decision(repo_root: &Path, path: &str) -> Option<IgnoreDecision> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["check-ignore", "--no-index", "-v", path])
        .output()
        .ok()?;

    match output.status.code() {
        Some(0) => parse_git_check_ignore(&output.stdout),
        Some(1) => Some(IgnoreDecision::Allowed),
        _ => None,
    }
}

fn parse_git_check_ignore(stdout: &[u8]) -> Option<IgnoreDecision> {
    let output = String::from_utf8_lossy(stdout);
    let line = output.lines().last()?;
    let (source, rest) = line.split_once(':')?;
    if source != ".gitignore" {
        return None;
    }
    let (_, pattern_and_path) = rest.split_once(':')?;
    let (pattern, _) = pattern_and_path.split_once('\t')?;
    if pattern.starts_with('!') {
        Some(IgnoreDecision::Allowed)
    } else {
        Some(IgnoreDecision::Ignored)
    }
}
