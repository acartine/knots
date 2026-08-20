//! Codex-specific managed-skill install and doctor coverage.
//!
//! Split out of `managed_skills_tests.rs` to keep both files under the
//! 500-line limit.

use std::fs;

use super::tests::{env_lock, unique_root};
use super::*;

#[test]
fn codex_install_uses_project_agents_only() {
    let repo_ws = unique_root("managed-skills-codex-project");
    let repo = repo_ws.path().to_path_buf();
    let home_ws = unique_root("managed-skills-home");
    let home = home_ws.path().to_path_buf();
    fs::create_dir_all(repo.join(".agents")).expect("agents root");
    let out = install_missing(&repo, Some(&home), SkillTool::Codex).expect("install");
    assert!(out.contains("installed"));
    assert!(repo.join(".agents/skills/knots/SKILL.md").exists());

    // Creates .agents/skills when .agents is absent
    let repo2_ws = unique_root("managed-skills-codex-create");
    let repo2 = repo2_ws.path().to_path_buf();
    let home2_ws = unique_root("managed-skills-home");
    let home2 = home2_ws.path().to_path_buf();
    let out = install_missing(&repo2, Some(&home2), SkillTool::Codex).expect("install");
    assert!(out.contains("installed"));
    assert!(repo2.join(".agents/skills/knots/SKILL.md").exists());
}

#[test]
fn doctor_detects_and_fixes_project_level_codex_skills() {
    let _guard = env_lock().lock().expect("env lock");
    let repo_ws = unique_root("managed-skills-codex-doctor-project");
    let repo = repo_ws.path().to_path_buf();
    let home_ws = unique_root("managed-skills-home");
    let home = home_ws.path().to_path_buf();
    let prior_home = std::env::var_os("HOME");
    fs::create_dir_all(repo.join(".agents")).expect("agents root");
    std::env::set_var("HOME", &home);

    let check = doctor_check(&repo, Some(&home), SkillTool::Codex);
    assert_eq!(check.status, DoctorStatus::Warn);
    assert!(check.detail.contains(".agents/skills"));

    install_missing(&repo, Some(&home), SkillTool::Codex).expect("install");
    assert!(repo.join(".agents/skills/knots/SKILL.md").exists());
    let check = doctor_check(&repo, Some(&home), SkillTool::Codex);
    assert_eq!(check.status, DoctorStatus::Pass);

    let knots = repo.join(".agents/skills/knots/SKILL.md");
    fs::write(&knots, "stale").expect("stale");
    let c = doctor_check(&repo, Some(&home), SkillTool::Codex);
    assert_eq!(c.status, DoctorStatus::Warn);
    fix_doctor_check(&repo, "skills_codex");
    let after = doctor_check(&repo, Some(&home), SkillTool::Codex);
    assert_eq!(after.status, DoctorStatus::Pass);
    assert!(fs::read_to_string(&knots).expect("read").contains("---"));

    match prior_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
}
