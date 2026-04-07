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

    write_skills(
        &SkillLocation {
            scope: LocationScope::Project,
            tool_root: repo_root.join(".claude"),
            skills_root: repo_root.join(".claude/skills"),
        },
        managed_skills(),
    )
    .expect("project skills should write");

    let output = run_command_with_io(
        &repo_root,
        Some(&home),
        false,
        SkillsCommand::Uninstall(SkillTool::Claude),
    )
    .expect("uninstall should succeed");

    assert!(output.contains(".claude/skills/knots/SKILL.md"));
    assert!(!repo_root.join(".claude/skills/knots/SKILL.md").exists());
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

    let check = doctor_check(&repo_root, Some(&home), SkillTool::Claude);

    assert_eq!(check.status, DoctorStatus::Warn);
    assert!(check.detail.contains(".claude/skills"));
    assert!(check.detail.contains("run `kno skills install claude`"));
}

#[test]
fn doctor_warns_when_preferred_destination_has_drifted_skills() {
    let repo_root = unique_root("managed-skills-doctor-drift");
    let home = unique_root("managed-skills-home");
    let codex_root = home.join(".codex");
    fs::create_dir_all(&codex_root).expect("codex root");
    install_missing(&repo_root, Some(&home), SkillTool::Codex).expect("install");
    let knots = codex_root.join("skills/knots/SKILL.md");
    fs::write(&knots, "stale").expect("write stale");

    let check = doctor_check(&repo_root, Some(&home), SkillTool::Codex);
    assert_eq!(check.status, DoctorStatus::Warn);
    assert!(check.detail.contains("drift detected"));
    assert!(check.detail.contains(knots.to_string_lossy().as_ref()));
    assert!(check.detail.contains("run `kno skills update codex`"));
}

#[test]
fn doctor_warns_when_preferred_destination_has_missing_and_drifted_skills() {
    let repo_root = unique_root("managed-skills-doctor-mixed");
    let home = unique_root("managed-skills-home");
    let codex_root = home.join(".codex");
    fs::create_dir_all(&codex_root).expect("codex root");
    install_missing(&repo_root, Some(&home), SkillTool::Codex).expect("install");
    let knots = codex_root.join("skills/knots/SKILL.md");
    let knots_e2e = codex_root.join("skills/knots-e2e/SKILL.md");
    fs::write(&knots, "stale").expect("write stale");
    fs::remove_file(&knots_e2e).expect("remove e2e skill");

    let check = doctor_check(&repo_root, Some(&home), SkillTool::Codex);
    assert_eq!(check.status, DoctorStatus::Warn);
    assert!(check.detail.contains("missing"));
    assert!(check.detail.contains("drifted"));
    assert!(check.detail.contains(knots.to_string_lossy().as_ref()));
    assert!(check.detail.contains(knots_e2e.to_string_lossy().as_ref()));
    assert!(check
        .detail
        .contains("run `kno skills install codex` then `kno skills update codex`"));
}

#[test]
fn doctor_treats_unreadable_existing_skill_as_drift() {
    let repo_root = unique_root("managed-skills-doctor-unreadable");
    let home = unique_root("managed-skills-home");
    let codex_root = home.join(".codex");
    fs::create_dir_all(&codex_root).expect("codex root");
    install_missing(&repo_root, Some(&home), SkillTool::Codex).expect("install");
    let knots = codex_root.join("skills/knots/SKILL.md");
    fs::remove_file(&knots).expect("remove knots skill");
    fs::create_dir_all(&knots).expect("replace with directory");

    let check = doctor_check(&repo_root, Some(&home), SkillTool::Codex);
    assert_eq!(check.status, DoctorStatus::Warn);
    assert!(check.detail.contains("drift detected"));
    assert!(check.detail.contains(knots.to_string_lossy().as_ref()));
    assert!(check.detail.contains("run `kno skills update codex`"));
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
    assert!(knots.contains("kno -C <path_to_repo>"));
    assert!(knots.contains("Knots is installed for the repo root"));
    assert!(knots.contains("If the claimed knot lists children"));
    assert!(knots.contains("If every child advanced"));
    assert!(knots.contains("If any child rolled back"));

    let knots_e2e = render_skill(managed_skills()[1]);
    assert!(knots_e2e.contains("kno -C <path_to_repo>"));
    assert!(knots_e2e.contains("Knots is installed for the repo root"));
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
fn doctor_fix_reconciles_drifted_skills_for_detected_root() {
    let _guard = env_lock().lock().expect("env lock");
    let repo_root = unique_root("managed-skills-fix");
    let home = unique_root("managed-skills-home");
    let prior_home = std::env::var_os("HOME");
    fs::create_dir_all(home.join(".codex")).expect("codex root");
    std::env::set_var("HOME", &home);

    install_missing(&repo_root, Some(&home), SkillTool::Codex).expect("install");
    let knots = home.join(".codex/skills/knots/SKILL.md");
    fs::write(&knots, "stale").expect("write stale");
    assert_eq!(
        doctor_check(&repo_root, Some(&home), SkillTool::Codex).status,
        DoctorStatus::Warn
    );

    fix_doctor_check(&repo_root, "skills_codex");
    assert_eq!(
        doctor_check(&repo_root, Some(&home), SkillTool::Codex).status,
        DoctorStatus::Pass
    );
    assert!(fs::read_to_string(&knots).unwrap().contains("---"));

    match prior_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
}

#[test]
fn skill_tool_helpers_cover_display_and_lookup_paths() {
    assert_eq!(SkillTool::Codex.slug(), "codex");
    assert_eq!(SkillTool::Claude.to_string(), "Claude");
    assert_eq!(SkillTool::OpenCode.doctor_check_name(), "skills_opencode");
    assert_eq!(expected_root_hint(SkillTool::Codex), ".agents or ~/.codex");
    assert_eq!(expected_root_hint(SkillTool::Claude), "./.claude");
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
    fs::create_dir_all(home.join(".codex")).expect("codex user root");
    fs::create_dir_all(home.join(".config/opencode")).expect("opencode user root");

    fs::create_dir_all(repo_root.join(".agents")).expect("codex project root");

    assert_eq!(SkillTool::Codex.locations(&repo_root, Some(&home)).len(), 2);
    assert_eq!(
        SkillTool::Claude.locations(&repo_root, Some(&home)).len(),
        1
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
    assert!(checks[0].detail.contains(".agents/skills"));
}

#[test]
fn install_reports_already_installed_when_nothing_is_missing() {
    let repo_root = unique_root("managed-skills-install-existing");
    let home = unique_root("managed-skills-home");
    fs::create_dir_all(home.join(".codex")).expect("codex root");
    install_missing(&repo_root, Some(&home), SkillTool::Codex).expect("initial install");
    let output = install_missing(&repo_root, Some(&home), SkillTool::Codex).expect("second");
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
    fs::write(&knots, "stale").expect("write stale");
    let output = update_managed(&repo_root, Some(&home), false, SkillTool::Codex).expect("update");
    assert!(output.contains("updated"));
    assert!(output.contains(".codex/skills/knots/SKILL.md"));
    assert!(fs::read_to_string(knots).unwrap().contains("---"));
}

#[test]
fn update_only_writes_to_preferred_location_not_user_level() {
    let repo_root = unique_root("managed-skills-update-scope");
    let home = unique_root("managed-skills-home");
    fs::create_dir_all(repo_root.join(".claude")).expect("project root");

    let project_loc = SkillLocation {
        scope: LocationScope::Project,
        tool_root: repo_root.join(".claude"),
        skills_root: repo_root.join(".claude/skills"),
    };
    write_skills(&project_loc, managed_skills()).expect("project install");

    let project_knots = repo_root.join(".claude/skills/knots/SKILL.md");
    fs::write(&project_knots, "stale").expect("project stale");

    let output = update_managed(&repo_root, Some(&home), false, SkillTool::Claude).expect("update");

    assert!(output.contains("updated"));
    assert!(fs::read_to_string(&project_knots)
        .expect("project knots")
        .contains("---"));
}

#[test]
fn claude_ignores_user_level_root_even_when_home_is_set() {
    let repo_root = unique_root("managed-skills-claude-project-only");
    let home = unique_root("managed-skills-home");
    fs::create_dir_all(home.join(".claude")).expect("user root");

    let check = doctor_check(&repo_root, Some(&home), SkillTool::Claude);
    assert_eq!(check.status, DoctorStatus::Warn);
    assert!(check.detail.contains("Claude root not detected"));
    assert!(check.detail.contains("create ./.claude"));

    let err = run_command_with_io(
        &repo_root,
        Some(&home),
        false,
        SkillsCommand::Install(SkillTool::Claude),
    )
    .expect_err("install should fail without a project-level root");

    assert!(err.to_string().contains("create ./.claude first"));
    assert!(!home.join(".claude/skills/knots/SKILL.md").exists());
}

#[test]
fn prompt_install_missing_accepts_yes_and_rejects_no() {
    let dest = SkillLocation {
        scope: LocationScope::User,
        tool_root: PathBuf::from("/tmp/.codex"),
        skills_root: PathBuf::from("/tmp/.codex/skills"),
    };
    let missing = vec![managed_skills()[0], managed_skills()[1]];

    let mut buf = Vec::new();
    let mut yes = std::io::Cursor::new("yes\n");
    let ok = prompt_install_missing(&mut buf, &mut yes, SkillTool::Codex, &dest, &missing)
        .expect("prompt should succeed");
    assert!(ok);
    let text = String::from_utf8(buf).expect("utf8");
    assert!(text.contains("/tmp/.codex/skills/knots/SKILL.md"));
    assert!(text.contains("/tmp/.codex/skills/knots-e2e/SKILL.md"));

    let mut buf = Vec::new();
    let mut no = std::io::Cursor::new("n\n");
    let ok = prompt_install_missing(&mut buf, &mut no, SkillTool::Codex, &dest, &missing)
        .expect("prompt should succeed");
    assert!(!ok);
}

#[test]
fn helper_functions_cover_empty_and_missing_paths() {
    let repo_root = unique_root("managed-skills-helpers");
    let home = unique_root("managed-skills-home");
    let preferred = preferred_location(&repo_root, Some(&home), SkillTool::Codex)
        .expect("preferred location should resolve to the user root");
    assert_eq!(preferred.tool_root, home.join(".codex"));
    let project_fallback = preferred_location(&repo_root, None, SkillTool::Codex)
        .expect("preferred location should resolve to project .agents");
    assert_eq!(project_fallback.tool_root, repo_root.join(".agents"));

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
    assert!(!home.join(".codex/skills/knots/SKILL.md").exists());

    fix_doctor_check(&repo_root, "skills_codex");
    assert_eq!(
        doctor_check(&repo_root, Some(&home), SkillTool::Codex).status,
        DoctorStatus::Pass
    );
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

    let install =
        run_command(&repo_root, SkillsCommand::Install(SkillTool::Codex)).expect("install");
    assert!(install.contains("installed"));
    assert!(doctor_checks(&repo_root)
        .iter()
        .any(|c| c.status == DoctorStatus::Pass));

    let knots = home.join(".codex/skills/knots/SKILL.md");
    fs::remove_file(&knots).expect("remove knots");
    fix_doctor_check(&repo_root, "skills_codex");
    assert!(knots.exists());
    fix_doctor_check(&repo_root, "unknown");

    match prior_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
}

#[test]
fn codex_install_prefers_project_agents_and_falls_back_to_user() {
    let project = unique_root("managed-skills-codex-project");
    let fallback = unique_root("managed-skills-codex-fallback");
    let home_a = unique_root("managed-skills-home");
    let home_b = unique_root("managed-skills-home");
    fs::create_dir_all(project.join(".agents")).expect("agents dir");
    fs::create_dir_all(home_a.join(".codex")).expect("codex user root");
    fs::create_dir_all(home_b.join(".codex")).expect("codex user root");

    let out = install_missing(&project, Some(&home_a), SkillTool::Codex).expect("project install");
    assert!(out.contains("installed"));
    assert!(project.join(".agents/skills/knots/SKILL.md").exists());
    assert!(!home_a.join(".codex/skills/knots/SKILL.md").exists());

    let out =
        install_missing(&fallback, Some(&home_b), SkillTool::Codex).expect("fallback install");
    assert!(out.contains("installed"));
    assert!(home_b.join(".codex/skills/knots/SKILL.md").exists());
    assert!(!fallback.join(".agents/skills/knots/SKILL.md").exists());
}

#[test]
fn doctor_and_fix_work_with_project_level_codex_skills() {
    let _guard = env_lock().lock().expect("env lock");
    let repo_root = unique_root("managed-skills-codex-proj-dr");
    let home = unique_root("managed-skills-home");
    let prior_home = std::env::var_os("HOME");
    fs::create_dir_all(repo_root.join(".agents")).expect("agents dir");
    std::env::set_var("HOME", &home);

    install_missing(&repo_root, Some(&home), SkillTool::Codex).expect("install");
    let check = doctor_check(&repo_root, Some(&home), SkillTool::Codex);
    assert_eq!(check.status, DoctorStatus::Pass);
    assert!(check.detail.contains(".agents/skills"));

    let knots = repo_root.join(".agents/skills/knots/SKILL.md");
    fs::write(&knots, "stale").expect("write stale");
    assert_eq!(
        doctor_check(&repo_root, Some(&home), SkillTool::Codex).status,
        DoctorStatus::Warn
    );

    fix_doctor_check(&repo_root, "skills_codex");
    let after = doctor_check(&repo_root, Some(&home), SkillTool::Codex);
    assert_eq!(after.status, DoctorStatus::Pass);
    assert!(fs::read_to_string(&knots).unwrap().contains("---"));

    match prior_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
}
