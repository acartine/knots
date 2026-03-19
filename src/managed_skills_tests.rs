use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use super::*;

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn unique_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("{label}-{}", uuid::Uuid::now_v7()));
    fs::create_dir_all(&root).expect("temp root should be creatable");
    root
}

#[test]
fn install_prefers_project_location_when_supported() {
    let repo_root = unique_root("managed-skills-install");
    let home = unique_root("managed-skills-home");
    fs::create_dir_all(repo_root.join(".claude")).expect("project root");
    fs::create_dir_all(home.join(".claude")).expect("user root");

    let output = run_command_with_io(
        &repo_root,
        Some(&home),
        false,
        SkillsCommand::Install(SkillTool::Claude),
    )
    .expect("install should succeed");

    assert!(output.contains("installed"));
    assert!(output.contains(".claude/skills/knots/SKILL.md"));
    assert!(repo_root.join(".claude/skills/knots/SKILL.md").exists());
    assert!(!home.join(".claude/skills/knots/SKILL.md").exists());
}

#[test]
fn uninstall_removes_installed_skills_from_all_detected_locations() {
    let repo_root = unique_root("managed-skills-uninstall");
    let home = unique_root("managed-skills-home");
    fs::create_dir_all(repo_root.join(".claude")).expect("project root");
    fs::create_dir_all(home.join(".claude")).expect("user root");

    write_skills(
        &SkillLocation {
            scope: LocationScope::Project,
            tool_root: repo_root.join(".claude"),
            skills_root: repo_root.join(".claude/skills"),
        },
        managed_skills(),
    )
    .expect("project skills should write");
    write_skills(
        &SkillLocation {
            scope: LocationScope::User,
            tool_root: home.join(".claude"),
            skills_root: home.join(".claude/skills"),
        },
        managed_skills(),
    )
    .expect("user skills should write");

    let output = run_command_with_io(
        &repo_root,
        Some(&home),
        false,
        SkillsCommand::Uninstall(SkillTool::Claude),
    )
    .expect("uninstall should succeed");

    assert!(output.contains(".claude/skills/knots/SKILL.md"));
    assert!(!repo_root.join(".claude/skills/knots/SKILL.md").exists());
    assert!(!home.join(".claude/skills/knots/SKILL.md").exists());
}

#[test]
fn update_requires_install_in_noninteractive_mode_when_skills_are_missing() {
    let repo_root = unique_root("managed-skills-update");
    let home = unique_root("managed-skills-home");
    fs::create_dir_all(home.join(".codex")).expect("codex root");

    let err = run_command_with_io(
        &repo_root,
        Some(&home),
        false,
        SkillsCommand::Update(SkillTool::Codex),
    )
    .expect_err("update should fail without installed skills");

    assert!(err.to_string().contains("run `kno skills install codex`"));
}

#[test]
fn doctor_warns_when_preferred_destination_is_missing_skills() {
    let repo_root = unique_root("managed-skills-doctor");
    let home = unique_root("managed-skills-home");
    fs::create_dir_all(repo_root.join(".claude")).expect("project root");
    fs::create_dir_all(home.join(".claude")).expect("user root");
    write_skills(
        &SkillLocation {
            scope: LocationScope::User,
            tool_root: home.join(".claude"),
            skills_root: home.join(".claude/skills"),
        },
        managed_skills(),
    )
    .expect("user skills should write");

    let check = doctor_check(&repo_root, Some(&home), SkillTool::Claude);

    assert_eq!(check.status, DoctorStatus::Warn);
    assert!(check.detail.contains(".claude/skills"));
    assert!(check.detail.contains("run `kno skills install claude`"));
}

#[test]
fn render_skill_uses_hyphenated_deploy_name() {
    let rendered = render_skill(managed_skills()[1]);
    assert!(rendered.contains("name: knots-e2e"));
    assert!(rendered.contains("# Knots E2E"));
}

#[test]
fn managed_skills_describe_parent_child_workflow() {
    let knots = render_skill(managed_skills()[0]);
    assert!(knots.contains("If the claimed knot lists children"));
    assert!(knots.contains("If every child advanced"));
    assert!(knots.contains("If any child rolled back"));

    let knots_e2e = render_skill(managed_skills()[1]);
    assert!(knots_e2e.contains("If the claimed knot lists children"));
    assert!(knots_e2e.contains("advance the parent and continue the loop"));
    assert!(knots_e2e.contains("roll the parent back and stop"));
}

#[test]
fn managed_skill_inventory_contains_only_knots_skills() {
    let names = managed_skills()
        .iter()
        .map(|skill| skill.deploy_name)
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["knots", "knots-e2e"]);
}

#[test]
fn doctor_fix_installs_missing_skills_for_detected_root() {
    let repo_root = unique_root("managed-skills-fix");
    let home = unique_root("managed-skills-home");
    fs::create_dir_all(home.join(".codex")).expect("codex root");

    let before = doctor_check(&repo_root, Some(&home), SkillTool::Codex);
    assert_eq!(before.status, DoctorStatus::Warn);

    write_skills(
        &SkillLocation {
            scope: LocationScope::User,
            tool_root: home.join(".codex"),
            skills_root: home.join(".codex/skills"),
        },
        &missing_skills(&SkillLocation {
            scope: LocationScope::User,
            tool_root: home.join(".codex"),
            skills_root: home.join(".codex/skills"),
        }),
    )
    .expect("skills should write");

    let after = doctor_check(&repo_root, Some(&home), SkillTool::Codex);
    assert_eq!(after.status, DoctorStatus::Pass);
}

#[test]
fn skill_tool_helpers_cover_display_and_lookup_paths() {
    assert_eq!(SkillTool::Codex.slug(), "codex");
    assert_eq!(SkillTool::Claude.to_string(), "Claude");
    assert_eq!(SkillTool::OpenCode.doctor_check_name(), "skills_opencode");
    assert_eq!(
        expected_root_hint(SkillTool::Claude),
        ".claude or ~/.claude"
    );
    assert_eq!(
        expected_root_hint(SkillTool::OpenCode),
        ".opencode or ~/.config/opencode"
    );
    assert_eq!(tool_for_check_name("skills_codex"), Some(SkillTool::Codex));
    assert_eq!(tool_for_check_name("unknown"), None);
}

#[test]
fn locations_detect_supported_roots_for_all_tools() {
    let repo_root = unique_root("managed-skills-locations");
    let home = unique_root("managed-skills-home");
    fs::create_dir_all(repo_root.join(".claude")).expect("claude project root");
    fs::create_dir_all(repo_root.join(".opencode")).expect("opencode project root");
    fs::create_dir_all(home.join(".claude")).expect("claude user root");
    fs::create_dir_all(home.join(".codex")).expect("codex user root");
    fs::create_dir_all(home.join(".config/opencode")).expect("opencode user root");

    assert_eq!(SkillTool::Codex.locations(&repo_root, Some(&home)).len(), 1);
    assert_eq!(
        SkillTool::Claude.locations(&repo_root, Some(&home)).len(),
        2
    );
    assert_eq!(
        SkillTool::OpenCode.locations(&repo_root, Some(&home)).len(),
        2
    );
}

#[test]
fn doctor_checks_warn_when_roots_are_missing() {
    let repo_root = unique_root("managed-skills-doctor-missing");
    let checks = doctor_checks_with_home(&repo_root, None);

    assert_eq!(checks.len(), 3);
    assert!(checks
        .iter()
        .all(|check| check.status == DoctorStatus::Warn));
    assert!(checks[0].detail.contains("create ~/.codex"));
}

#[test]
fn install_reports_already_installed_when_nothing_is_missing() {
    let repo_root = unique_root("managed-skills-install-existing");
    let home = unique_root("managed-skills-home");
    fs::create_dir_all(home.join(".codex")).expect("codex root");

    install_missing(&repo_root, Some(&home), SkillTool::Codex).expect("initial install");
    let output =
        install_missing(&repo_root, Some(&home), SkillTool::Codex).expect("second install");

    assert!(output.contains("already installed"));
}

#[test]
fn uninstall_errors_when_no_managed_skills_are_installed() {
    let repo_root = unique_root("managed-skills-uninstall-empty");
    let home = unique_root("managed-skills-home");
    fs::create_dir_all(home.join(".codex")).expect("codex root");

    let err = uninstall_managed(&repo_root, Some(&home), SkillTool::Codex)
        .expect_err("uninstall should fail");

    assert!(err.to_string().contains("no installed managed skills"));
}

#[test]
fn update_rewrites_existing_skills_when_install_is_complete() {
    let repo_root = unique_root("managed-skills-update-existing");
    let home = unique_root("managed-skills-home");
    let codex_root = home.join(".codex");
    fs::create_dir_all(&codex_root).expect("codex root");

    install_missing(&repo_root, Some(&home), SkillTool::Codex).expect("install");
    let knots = codex_root.join("skills/knots/SKILL.md");
    fs::write(&knots, "stale").expect("knots skill should be writable");

    let output = update_managed(&repo_root, Some(&home), false, SkillTool::Codex).expect("update");

    assert!(output.contains("updated"));
    assert!(output.contains(".codex/skills/knots/SKILL.md"));
    assert!(fs::read_to_string(knots)
        .expect("knots should exist")
        .contains("---"));
}

#[test]
fn prompt_install_missing_accepts_yes_and_rejects_no() {
    let destination = SkillLocation {
        scope: LocationScope::User,
        tool_root: PathBuf::from("/tmp/.codex"),
        skills_root: PathBuf::from("/tmp/.codex/skills"),
    };
    let missing = vec![managed_skills()[0], managed_skills()[1]];
    let mut output = Vec::new();
    let mut yes = std::io::Cursor::new("yes\n");
    let approved = prompt_install_missing(
        &mut output,
        &mut yes,
        SkillTool::Codex,
        &destination,
        &missing,
    )
    .expect("prompt should succeed");
    assert!(approved);
    let output = String::from_utf8(output).expect("utf8");
    assert!(output.contains("/tmp/.codex/skills/knots/SKILL.md"));
    assert!(output.contains("/tmp/.codex/skills/knots-e2e/SKILL.md"));

    let mut output = Vec::new();
    let mut no = std::io::Cursor::new("n\n");
    let approved = prompt_install_missing(
        &mut output,
        &mut no,
        SkillTool::Codex,
        &destination,
        &missing,
    )
    .expect("prompt should succeed");
    assert!(!approved);
}

#[test]
fn helper_functions_cover_empty_and_missing_paths() {
    let repo_root = unique_root("managed-skills-helpers");
    let home = unique_root("managed-skills-home");
    let preferred = preferred_location(&repo_root, Some(&home), SkillTool::Codex)
        .expect("preferred location should resolve to the user root");
    assert_eq!(preferred.tool_root, home.join(".codex"));
    let missing = preferred_location(&repo_root, None, SkillTool::Codex)
        .expect_err("preferred location should fail without HOME");
    assert!(missing.to_string().contains("create ~/.codex first"));

    let empty_location = SkillLocation {
        scope: LocationScope::User,
        tool_root: home.join(".codex"),
        skills_root: home.join(".codex/skills"),
    };
    write_skills(&empty_location, &[]).expect("empty writes should succeed");
    remove_dir_if_empty(&empty_location.skills_root).expect("missing dirs should be ignored");
    assert!(installed_locations(&repo_root, Some(&home), SkillTool::Codex).is_empty());
}

#[test]
fn doctor_fix_installs_missing_skills_when_user_root_is_absent() {
    let _guard = env_lock().lock().expect("env lock");
    let repo_root = unique_root("managed-skills-fix-missing-root");
    let home = unique_root("managed-skills-home");
    let prior_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &home);

    let before = doctor_check(&repo_root, Some(&home), SkillTool::Codex);
    assert_eq!(before.status, DoctorStatus::Warn);
    assert!(before.detail.contains(".codex/skills"));
    assert!(!home.join(".codex/skills/knots/SKILL.md").exists());

    fix_doctor_check(&repo_root, "skills_codex");

    let after = doctor_check(&repo_root, Some(&home), SkillTool::Codex);
    assert_eq!(after.status, DoctorStatus::Pass);
    assert!(home.join(".codex/skills/knots/SKILL.md").exists());

    match prior_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
}

#[test]
fn public_environment_based_helpers_use_home_env() {
    let _guard = env_lock().lock().expect("env lock");
    let repo_root = unique_root("managed-skills-public");
    let home = unique_root("managed-skills-home");
    fs::create_dir_all(home.join(".codex")).expect("codex root");

    let prior_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &home);

    let install = run_command(&repo_root, SkillsCommand::Install(SkillTool::Codex))
        .expect("install should succeed");
    assert!(install.contains("installed"));

    let checks = doctor_checks(&repo_root);
    assert!(checks
        .iter()
        .any(|check| check.status == DoctorStatus::Pass));

    let knots = home.join(".codex/skills/knots/SKILL.md");
    fs::remove_file(&knots).expect("knots skill should exist");
    fix_doctor_check(&repo_root, "skills_codex");
    assert!(knots.exists());
    fix_doctor_check(&repo_root, "unknown");

    match prior_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
}
