use std::fs;
use std::path::PathBuf;
use std::process::Command;

use super::{
    doctor_check, fix_doctor_check, install_missing, managed_skills, render_skill, update_managed,
    DoctorStatus, SkillTool,
};

pub(super) fn unique_root(label: &str) -> knots_test_support::TestWorkspace {
    knots_test_support::workspace(label)
}

pub(super) fn init_git_repo(root: &PathBuf) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("init")
        .output()
        .expect("git init should run");
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(super) fn check_ignore(root: &PathBuf, path: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["check-ignore", "--quiet", path])
        .status()
        .expect("git check-ignore should run")
        .success()
}

fn run_git(root: &PathBuf, args: &[&str]) {
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

fn commit_all(root: &PathBuf, message: &str) {
    run_git(root, &["config", "user.name", "Knots Tests"]);
    run_git(
        root,
        &["config", "user.email", "knots-tests@example.invalid"],
    );
    run_git(root, &["add", "."]);
    run_git(root, &["commit", "-m", message]);
}

#[test]
fn managed_skill_inventory_includes_knots_create() {
    let names = managed_skills()
        .iter()
        .map(|skill| skill.deploy_name)
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            "knots",
            "knots-e2e",
            "knots-create",
            "knots-plan-orchestrator"
        ]
    );
}

#[test]
fn knots_create_skill_describes_structured_creation_inputs() {
    let skill = managed_skills()
        .iter()
        .copied()
        .find(|skill| skill.deploy_name == "knots-create")
        .expect("knots-create should be managed");
    let rendered = render_skill(skill);

    assert!(rendered.contains("name: knots-create"));
    assert!(rendered.contains("Put the"));
    assert!(rendered.contains("goal, verification steps, and constraints in `-d`"));
    assert!(rendered.contains("Put only numbered"));
    assert!(rendered.contains("acceptance criteria in `--acceptance`"));
    assert!(rendered.contains("Goal:"));
    assert!(rendered.contains("Verification:"));
    assert!(rendered.contains("Constraints:"));
    assert!(rendered.contains("exact commands or"));
    assert!(rendered.contains("UI actions"));
    assert!(rendered.contains("API routes"));
    assert!(rendered.contains("file paths"));
    assert!(rendered.contains("kno new \"<title>\""));
    assert!(rendered.contains("--acceptance"));
}

#[test]
fn managed_skills_document_lease_identity_and_handoff_capsules() {
    let by_name = |name: &str| {
        let skill = managed_skills()
            .iter()
            .copied()
            .find(|skill| skill.deploy_name == name)
            .expect("managed skill should exist");
        render_skill(skill)
    };

    let knots = by_name("knots");
    assert!(knots.contains("kno lease create"));
    assert!(knots.contains("kno claim <id> --lease <lease-id>"));
    assert!(knots.contains("kno next <id> --expected-state <current_state> --lease <lease-id>"));
    assert!(knots.contains("kno update <id> -H \"<capsule>\" --lease <lease-id>"));
    assert!(knots.contains("Agent identity for notes, handoff capsules"));
    assert!(knots.contains("legacy `--*-agentname/model/version` identity flags"));
    assert!(knots.contains("deprecated, ignored by current `kno`"));

    let create = by_name("knots-create");
    assert!(create.contains("kno lease create"));
    assert!(create.contains("--lease <lease-id>"));
    assert!(create.contains("kno update <id> -H \"<capsule>\" --lease <lease-id>"));
    assert!(create.contains("Authorship for notes"));
    assert!(create.contains("comes from the bound lease"));
    assert!(create.contains("`[unknown <date>]`"));
    assert!(create.contains("legacy `--*-agentname/model/version` identity flags"));

    let e2e = by_name("knots-e2e");
    assert!(e2e.contains("kno claim --e2e <id> --lease <lease-id>"));
    assert!(e2e.contains("kno next <id> --expected-state <current_state> --lease <lease-id>"));
    assert!(e2e.contains("kno update <id> -H \"<capsule>\" --lease <lease-id>"));
    assert!(e2e.contains("transitions, and gate decisions"));
    assert!(e2e.contains("deprecated, ignored by current `kno`"));

    let orchestrator = by_name("knots-plan-orchestrator");
    assert!(orchestrator.contains("kno lease create"));
    assert!(orchestrator.contains("claim its assigned knot with `--lease <lease-id>`"));
    assert!(orchestrator.contains("metadata authorship"));
    assert!(orchestrator.contains("kno update <plan-id> -H \"<capsule>\" --lease <lease-id>"));
}

#[test]
fn managed_skills_document_claim_lease_lifecycle() {
    let by_name = |name: &str| {
        let skill = managed_skills()
            .iter()
            .copied()
            .find(|skill| skill.deploy_name == name)
            .expect("managed skill should exist");
        render_skill(skill)
    };

    let knots = by_name("knots");
    assert!(knots.contains("a lease is a claim-scoped token"));
    assert!(knots.contains("release the claim"));
    assert!(knots.contains("lease `lease_terminated`"));
    assert!(knots.contains("Ordinary agents"));
    assert!(knots.contains("must not reuse or extend it"));
    assert!(knots.contains("Create or receive a fresh lease before claiming any later action"));

    let e2e = by_name("knots-e2e");
    assert!(e2e.contains("After every successful `kno next`, treat the lease id"));
    assert!(e2e.contains("kno claim --e2e <id> --lease <new-lease-id>"));
    assert!(e2e.contains("Do not try to reuse or extend the old lease"));
    assert!(
        !e2e.contains("Bind the same lease"),
        "e2e skill must not tell agents to reuse claim leases"
    );

    let create = by_name("knots-create");
    assert!(create.contains("not give future workers a reusable claim lease"));
    assert!(create.contains("marks it `lease_terminated`"));

    let orchestrator = by_name("knots-plan-orchestrator");
    assert!(orchestrator.contains("Worker leases follow the same claim-scoped lifecycle"));
    assert!(orchestrator.contains("fresh lease id for that claim"));
    assert!(orchestrator.contains("Do not reuse or extend a lease after it has been"));
}

#[test]
fn managed_skills_document_dead_lease_recovery() {
    let by_name = |name: &str| {
        let skill = managed_skills()
            .iter()
            .copied()
            .find(|skill| skill.deploy_name == name)
            .expect("managed skill should exist");
        render_skill(skill)
    };

    let knots = by_name("knots");
    assert!(knots.contains("## Dead-lease recovery"));
    assert!(knots.contains("terminated or expired"));
    assert!(knots.contains("queue state (`ready_for_*`)"));
    assert!(knots.contains("kno claim <id> --lease <new-lease-id>"));
    assert!(knots.contains("held by an active lease, stop"));
    assert!(knots.contains("kno state --force"));

    let e2e = by_name("knots-e2e");
    assert!(e2e.contains("## Dead-lease recovery"));
    assert!(e2e.contains("kno claim --e2e <id> --lease <new-lease-id>"));
    assert!(e2e.contains("resume the e2e loop only after that claim succeeds"));
    assert!(e2e.contains("action state held by an active"));
    assert!(e2e.contains("lease, stop"));
    assert!(e2e.contains("kno state --force"));
}

#[test]
fn knots_plan_orchestrator_skill_describes_plan_execution_protocol() {
    let skill = managed_skills()
        .iter()
        .copied()
        .find(|skill| skill.deploy_name == "knots-plan-orchestrator")
        .expect("knots-plan-orchestrator should be managed");
    let rendered = render_skill(skill);

    assert!(rendered.contains("name: knots-plan-orchestrator"));
    assert!(rendered.contains("# Knots Plan Orchestrator"));
    assert!(rendered.contains("kno show <plan-id> --json"));
    assert!(rendered.contains("execution_plan"));
    assert!(rendered.contains("Waves are sequential"));
    assert!(rendered.contains("Steps within a wave are sequential"));
    assert!(rendered.contains("Knots within a step are concurrent"));
    assert!(rendered.contains("kno show <knot-id> --json"));
    assert!(rendered.contains("kno next <plan-id>"));
    assert!(rendered.contains("kno rollback <plan-id>"));
    assert!(rendered.contains("kno -C <path_to_repo>"));
    assert!(rendered.contains("SHIPPED"));
    assert!(rendered.contains("BLOCKED"));
    assert!(rendered.contains("DEFERRED"));
    assert!(rendered.contains("your own protocol for launching and managing coding"));
}

#[test]
fn update_prints_commit_notice_for_tracked_skill_changes() {
    let repo_ws = unique_root("managed-skills-update-commit-notice");
    let repo = repo_ws.path().to_path_buf();
    let home_ws = unique_root("managed-skills-home");
    let home = home_ws.path().to_path_buf();
    init_git_repo(&repo);
    fs::create_dir_all(repo.join(".agents")).expect("agents root");
    install_missing(&repo, Some(&home), SkillTool::Codex).expect("install");

    let stale_skill = repo.join(".agents/skills/knots-e2e/SKILL.md");
    fs::write(&stale_skill, "stale managed skill\n").expect("stale skill");
    commit_all(&repo, "commit stale managed skills");

    let output = update_managed(&repo, Some(&home), false, SkillTool::Codex).expect("update");
    let (_, notice) = output
        .split_once("Note: managed skill updates changed files")
        .expect("commit notice should be appended");

    assert!(notice.contains("commit them"));
    assert!(notice.contains("- .agents/skills/knots-e2e/SKILL.md"));
    assert!(
        !notice.contains("- .agents/skills/knots/SKILL.md"),
        "notice should list git-reported changes, not every rewritten skill"
    );
}

#[test]
fn update_skips_commit_notice_when_tracked_skills_are_clean() {
    let repo_ws = unique_root("managed-skills-update-clean-notice");
    let repo = repo_ws.path().to_path_buf();
    let home_ws = unique_root("managed-skills-home");
    let home = home_ws.path().to_path_buf();
    init_git_repo(&repo);
    fs::create_dir_all(repo.join(".agents")).expect("agents root");
    install_missing(&repo, Some(&home), SkillTool::Codex).expect("install");
    commit_all(&repo, "commit canonical managed skills");

    let output = update_managed(&repo, Some(&home), false, SkillTool::Codex).expect("update");

    assert!(output.contains("updated"));
    assert!(!output.contains("commit them"));
}

#[test]
fn update_skips_commit_notice_outside_git_repo() {
    let repo_ws = unique_root("managed-skills-update-nonrepo-notice");
    let repo = repo_ws.path().to_path_buf();
    let home_ws = unique_root("managed-skills-home");
    let home = home_ws.path().to_path_buf();
    fs::create_dir_all(repo.join(".agents")).expect("agents root");
    install_missing(&repo, Some(&home), SkillTool::Codex).expect("install");

    let output = update_managed(&repo, Some(&home), false, SkillTool::Codex).expect("update");

    assert!(output.contains("updated"));
    assert!(!output.contains("commit them"));
}

#[test]
fn opencode_install_bootstraps_agents_gitignore_and_cleans_legacy_locations() {
    let repo_ws = unique_root("managed-skills-opencode-install");
    let repo = repo_ws.path().to_path_buf();
    let home_ws = unique_root("managed-skills-home");
    let home = home_ws.path().to_path_buf();
    init_git_repo(&repo);
    fs::create_dir_all(repo.join(".opencode/skills/knots")).expect("legacy opencode project root");
    fs::write(repo.join(".opencode/skills/knots/SKILL.md"), "legacy").expect("legacy project");
    fs::create_dir_all(home.join(".config/opencode/skills/knots")).expect("legacy user root");
    fs::write(
        home.join(".config/opencode/skills/knots/SKILL.md"),
        "legacy",
    )
    .expect("legacy user");

    let output = install_missing(&repo, Some(&home), SkillTool::OpenCode).expect("install");
    assert!(output.contains(".agents/skills/knots/SKILL.md"));
    assert!(repo.join(".agents/skills/knots/SKILL.md").exists());
    assert!(!repo.join(".opencode/skills/knots/SKILL.md").exists());
    assert!(!home.join(".config/opencode/skills/knots/SKILL.md").exists());

    let gitignore = fs::read_to_string(repo.join(".gitignore")).expect("gitignore should exist");
    assert!(gitignore.lines().any(|line| line.trim() == "/.agents/**"));
    assert!(gitignore.lines().any(|line| line.trim() == "!/.agents/"));
    assert!(gitignore
        .lines()
        .any(|line| line.trim() == "!/.agents/skills/"));
    assert!(gitignore
        .lines()
        .any(|line| line.trim() == "!/.agents/skills/**"));
    fs::write(repo.join(".agents/private.txt"), "private").expect("private file");
    fs::create_dir_all(repo.join(".agents/worktrees/elastic-clarke-9c0ed3/.git"))
        .expect("nested worktree fixture");
    assert!(check_ignore(&repo, ".agents/private.txt"));
    assert!(check_ignore(
        &repo,
        ".agents/worktrees/elastic-clarke-9c0ed3/.git"
    ));
    assert!(!check_ignore(&repo, ".agents/skills/knots/SKILL.md"));
}

#[test]
fn claude_install_bootstraps_claude_gitignore_with_skills_allowlist() {
    let repo_ws = unique_root("managed-skills-claude-gitignore");
    let repo = repo_ws.path().to_path_buf();
    let home_ws = unique_root("managed-skills-home");
    let home = home_ws.path().to_path_buf();
    init_git_repo(&repo);
    fs::create_dir_all(repo.join(".claude")).expect("claude root");

    let output = install_missing(&repo, Some(&home), SkillTool::Claude).expect("install");
    assert!(output.contains(".claude/skills/knots/SKILL.md"));

    let gitignore = fs::read_to_string(repo.join(".gitignore")).expect("gitignore should exist");
    assert!(gitignore.lines().any(|line| line.trim() == "/.claude/**"));
    assert!(gitignore.lines().any(|line| line.trim() == "!/.claude/"));
    assert!(gitignore
        .lines()
        .any(|line| line.trim() == "!/.claude/skills/"));
    assert!(gitignore
        .lines()
        .any(|line| line.trim() == "!/.claude/skills/**"));
    fs::write(repo.join(".claude/settings.local.json"), "{}").expect("local settings");
    fs::create_dir_all(repo.join(".claude/worktrees/elastic-clarke-9c0ed3/.git"))
        .expect("nested worktree fixture");
    assert!(check_ignore(&repo, ".claude/settings.local.json"));
    assert!(check_ignore(
        &repo,
        ".claude/worktrees/elastic-clarke-9c0ed3/.git"
    ));
    assert!(!check_ignore(&repo, ".claude/skills/knots/SKILL.md"));
}

#[test]
fn doctor_warns_and_fixes_installed_claude_skills_with_broken_gitignore() {
    let repo_ws = unique_root("managed-skills-claude-gitignore-doctor");
    let repo = repo_ws.path().to_path_buf();
    let home_ws = unique_root("managed-skills-home");
    let home = home_ws.path().to_path_buf();
    init_git_repo(&repo);
    fs::create_dir_all(repo.join(".claude")).expect("claude root");

    install_missing(&repo, Some(&home), SkillTool::Claude).expect("install");
    fs::write(repo.join(".gitignore"), "/.claude/**\n!/.claude/skills/\n")
        .expect("broken gitignore should write");

    let check = doctor_check(&repo, Some(&home), SkillTool::Claude);
    assert_eq!(check.status, DoctorStatus::Warn);
    assert!(check
        .detail
        .contains(".gitignore does not blocklist .claude"));

    fix_doctor_check(&repo, "skills_claude");
    fs::create_dir_all(repo.join(".claude/worktrees/elastic-clarke-9c0ed3/.git"))
        .expect("nested worktree fixture");
    assert!(check_ignore(
        &repo,
        ".claude/worktrees/elastic-clarke-9c0ed3/.git"
    ));
    assert!(!check_ignore(&repo, ".claude/skills/knots/SKILL.md"));
    let check = doctor_check(&repo, Some(&home), SkillTool::Claude);
    assert_eq!(check.status, DoctorStatus::Pass);
}
