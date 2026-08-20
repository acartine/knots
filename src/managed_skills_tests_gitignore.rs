//! Managed-skill drift repair and its interaction with `.gitignore`.
//!
//! Split out of `managed_skills_tests_ext.rs` to keep both files under the
//! 500-line limit.

use std::fs;

use super::tests::env_lock;
use super::tests_ext::{check_ignore, init_git_repo, unique_root};
use super::*;

#[test]
fn doctor_fix_updates_drifted_skills_without_rewriting_effective_gitignore() {
    let repo_ws = unique_root("managed-skills-drift-no-gitignore-rewrite");
    let repo = repo_ws.path().to_path_buf();
    let home_ws = unique_root("managed-skills-home");
    let home = home_ws.path().to_path_buf();
    init_git_repo(&repo);
    fs::create_dir_all(repo.join(".agents")).expect("agents root");
    fs::create_dir_all(repo.join(".claude")).expect("claude root");

    install_missing(&repo, Some(&home), SkillTool::Codex).expect("codex install");
    install_missing(&repo, Some(&home), SkillTool::Claude).expect("claude install");

    let gitignore = "\
.claude/*
!.claude/skills/
!.claude/skills/**
/.agents/*
!/.agents/skills/
!/.agents/skills/**
";
    fs::write(repo.join(".gitignore"), gitignore).expect("gitignore should write");

    let e2e = managed_skills()
        .iter()
        .copied()
        .find(|skill| skill.deploy_name == "knots-e2e")
        .expect("knots-e2e should be managed");
    let agents_skill = repo.join(".agents/skills/knots-e2e/SKILL.md");
    let claude_skill = repo.join(".claude/skills/knots-e2e/SKILL.md");
    fs::write(&agents_skill, "stale").expect("agents skill should be writable");
    fs::write(&claude_skill, "stale").expect("claude skill should be writable");

    let codex = doctor_check(&repo, Some(&home), SkillTool::Codex);
    assert_eq!(codex.status, DoctorStatus::Warn);
    assert!(codex.detail.contains("managed skill drift detected"));
    assert!(!codex.detail.contains(".gitignore does not blocklist"));

    let checks = [
        crate::doctor::DoctorCheck::simple("skills_codex", DoctorStatus::Warn, "drift"),
        crate::doctor::DoctorCheck::simple("skills_claude", DoctorStatus::Warn, "drift"),
        crate::doctor::DoctorCheck::simple("skills_opencode", DoctorStatus::Warn, "drift"),
    ];
    let outcome = crate::doctor_fix::apply_fixes(&repo, &checks);
    assert!(!outcome.event_log_touched);

    assert_eq!(
        fs::read_to_string(&agents_skill).expect("agents skill"),
        render_skill(e2e)
    );
    assert_eq!(
        fs::read_to_string(&claude_skill).expect("claude skill"),
        render_skill(e2e)
    );
    assert_eq!(
        fs::read_to_string(repo.join(".gitignore")).expect("gitignore"),
        gitignore
    );
    assert!(check_ignore(&repo, ".agents/private.txt"));
    assert!(check_ignore(&repo, ".claude/settings.local.json"));
    assert!(!check_ignore(&repo, ".agents/skills/knots-e2e/SKILL.md"));
    assert!(!check_ignore(&repo, ".claude/skills/knots-e2e/SKILL.md"));
}

#[test]
fn doctor_skips_codex_and_opencode_when_agents_root_is_absent() {
    let _guard = env_lock().lock().expect("env lock");
    let repo_ws = unique_root("managed-skills-doctor-skip");
    let repo = repo_ws.path().to_path_buf();
    let home_ws = unique_root("managed-skills-home");
    let home = home_ws.path().to_path_buf();
    let prior_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &home);
    fs::create_dir_all(home.join(".config/opencode/skills/knots")).expect("legacy user root");
    fs::write(
        home.join(".config/opencode/skills/knots/SKILL.md"),
        "legacy",
    )
    .expect("legacy");

    let codex = doctor_check(&repo, Some(&home), SkillTool::Codex);
    assert_eq!(codex.status, DoctorStatus::Pass);
    let opencode = doctor_check(&repo, Some(&home), SkillTool::OpenCode);
    assert_eq!(opencode.status, DoctorStatus::Pass);

    fix_doctor_check(&repo, "skills_opencode");
    assert!(!home.join(".config/opencode/skills/knots/SKILL.md").exists());
    assert!(!repo.join(".agents/skills/knots/SKILL.md").exists());

    match prior_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
}
