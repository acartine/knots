use super::bundle_toml::render_json_bundle_from_toml;
use super::loom::CommandLoomBundleBuilder;
use super::operations::{read_bundle_source_with_builder, repo_config_path, write_repo_config};
use super::tests_helpers::{unique_workspace, SAMPLE_BUNDLE};
use super::*;
use crate::domain::knot_type::KnotType;

fn ensure_builtin_registry(root: &std::path::Path) -> WorkflowRepoConfig {
    ensure_builtin_workflows_registered(root).expect("builtin workflows should register")
}

#[test]
fn read_bundle_source_can_shell_out_to_loom() {
    let root_ws = unique_workspace("knots-installed-workflows-loom-dir");
    let root = root_ws.path().to_path_buf();
    let bin_dir = root.join("bin");
    let package_dir = root.join("pkg");
    std::fs::create_dir_all(&bin_dir).expect("bin dir should exist");
    std::fs::create_dir_all(&package_dir).expect("package dir should exist");
    std::fs::write(package_dir.join("loom.toml"), "name = 'pkg'").expect("loom.toml writes");

    let json_bundle = render_json_bundle_from_toml(SAMPLE_BUNDLE).expect("json render");
    let loom_script = format!(
        "#!/bin/sh\n\
         if [ \"$1\" = \"build\" ] && \
         [ \"$3\" = \"--emit\" ] && \
         [ \"$4\" = \"knots-bundle\" ]; then\n\
         cat <<'EOF'\n{json_bundle}\nEOF\n\
         else\nexit 1\nfi\n"
    );
    let loom_path = bin_dir.join("loom");
    std::fs::write(&loom_path, loom_script).expect("loom script writes");
    make_executable(&loom_path);

    // Point the builder directly at the fake binary's absolute path instead
    // of mutating process-global PATH, so this test cannot race the ~1280
    // other tests that spawn processes and share the same process-wide PATH.
    let builder = CommandLoomBundleBuilder::with_binary(&loom_path);
    let (raw, format) =
        read_bundle_source_with_builder(&package_dir, &builder).expect("loom package should build");
    assert!(matches!(format, BundleFormat::Json));
    assert!(raw.contains("\"format\": \"knots-bundle\""));
}

#[test]
fn registry_prefers_latest_version() {
    let root_ws = unique_workspace("knots-installed-workflows-latest");
    let root = root_ws.path().to_path_buf();
    let mut config = ensure_builtin_registry(&root);
    let v3 = root.join("custom-v3.toml");
    let v4 = root.join("custom-v4.toml");
    std::fs::write(&v3, SAMPLE_BUNDLE).expect("v3 writes");
    std::fs::write(&v4, SAMPLE_BUNDLE.replace("version = 3", "version = 4")).expect("v4 writes");
    install_bundle(&root, &v3).expect("v3 installs");
    install_bundle(&root, &v4).expect("v4 installs");
    config.register_workflow_for_knot_type(
        crate::domain::knot_type::KnotType::Work,
        WorkflowRef::new("custom_flow", None),
        true,
    );
    write_repo_config(&root, &config).expect("config writes");

    let registry = InstalledWorkflowRegistry::load(&root).expect("registry should load");
    let current = registry
        .current_workflow()
        .expect("current workflow resolves");
    assert_eq!(current.version, 4);
}

#[test]
fn set_selection_honors_explicit_profile() {
    let root_ws = unique_workspace("knots-installed-workflows-explicit-profile");
    let root = root_ws.path().to_path_buf();
    let source = root.join("custom-flow.toml");
    std::fs::write(&source, SAMPLE_BUNDLE).expect("bundle should write");
    install_bundle(&root, &source).expect("bundle should install");

    let config = set_current_workflow_selection(&root, "custom_flow", Some(3), Some("autopilot"))
        .expect("selection should succeed");
    assert_eq!(
        config
            .current_workflow_ref_for_knot_type(crate::domain::knot_type::KnotType::Work)
            .and_then(|workflow| workflow.version),
        Some(3)
    );
    assert_eq!(config.current_profile_id(), Some("custom_flow/autopilot"));
}

#[test]
fn set_default_profile_updates_repo_mapping() {
    let root_ws = unique_workspace("knots-installed-workflows-set-default-profile");
    let root = root_ws.path().to_path_buf();
    let source = root.join("custom-flow.toml");
    let bundle = SAMPLE_BUNDLE.replace(
        "[profiles.autopilot]\n\
         description = \"Custom profile\"\n\
         phases = [\"main\"]\n",
        "[profiles.autopilot]\n\
         description = \"Custom profile\"\n\
         phases = [\"main\"]\n\n\
         [profiles.beta]\n\
         description = \"Beta profile\"\n\
         phases = [\"main\"]\n",
    );
    std::fs::write(&source, bundle).expect("bundle should write");
    install_bundle(&root, &source).expect("bundle should install");

    let config = set_workflow_default_profile(&root, "custom_flow", Some("beta"))
        .expect("default profile should update");
    assert_eq!(
        config.default_profile_id_for_workflow("custom_flow"),
        Some("custom_flow/beta")
    );

    let registry = InstalledWorkflowRegistry::load(&root).expect("registry should load");
    assert_eq!(
        registry.default_profile_id_for_workflow("custom_flow"),
        Some("custom_flow/beta".to_string())
    );
}

#[test]
fn migrate_legacy_profiles_and_knot_type_map() {
    let root_ws = unique_workspace("knots-installed-workflows-repair-profiles");
    let root = root_ws.path().to_path_buf();
    let path = repo_config_path(&root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("config dir should exist");
    }
    std::fs::write(
        &path,
        r#"[knot_type_workflows.work.default]
workflow_id = "compatibility"
version = 1

[[knot_type_workflows.work.registered]]
workflow_id = "compatibility"
version = 1

[default_profiles]
compatibility = "human_gate"
custom_flow = "Reviewer"
other_flow = "Custom_Flow/Reviewer"
"#,
    )
    .expect("legacy config should write");

    let loaded = ensure_builtin_registry(&root);
    assert_eq!(
        loaded
            .current_workflow_ref_for_knot_type(KnotType::Work)
            .map(|workflow| workflow.workflow_id),
        Some("work_sdlc".to_string())
    );
    assert_eq!(
        loaded.default_profile_id_for_workflow("work_sdlc"),
        Some("semiauto")
    );
    assert_eq!(
        loaded.default_profile_id_for_workflow("custom_flow"),
        Some("reviewer")
    );
    assert_eq!(
        loaded.default_profile_id_for_workflow("other_flow"),
        Some("custom_flow/reviewer")
    );

    let repaired = std::fs::read_to_string(&path).expect("repaired config should read");
    assert!(repaired.contains("work_sdlc"));
    assert!(repaired.contains("semiauto"));
    assert!(repaired.contains("custom_flow/reviewer"));
}

#[test]
fn load_skips_non_version_and_loads_json_bundle() {
    let root_ws = unique_workspace("knots-installed-workflows-load-json");
    let root = root_ws.path().to_path_buf();
    ensure_builtin_registry(&root);
    let workflow_root = workflows_root(&root).join("legacy_flow");
    std::fs::create_dir_all(&workflow_root).expect("workflow root should exist");
    std::fs::write(workflow_root.join("README.txt"), "ignore me").expect("file should write");
    std::fs::create_dir_all(workflow_root.join("not-a-version")).expect("dir should exist");
    std::fs::create_dir_all(workflow_root.join("7")).expect("version dir should exist");
    let json_bundle =
        render_json_bundle_from_toml(&SAMPLE_BUNDLE.replace("custom_flow", "legacy_flow"))
            .expect("json bundle should render");
    std::fs::write(workflow_root.join("7/bundle.json"), json_bundle)
        .expect("json bundle should write");

    let registry = InstalledWorkflowRegistry::load(&root).expect("registry should load");
    let workflow = registry
        .require_workflow("legacy_flow")
        .expect("should load from bundle.json");
    assert_eq!(workflow.id, "legacy_flow");
    assert_eq!(workflow.version, 3);
}

#[test]
fn install_supports_json_input() {
    let root_ws = unique_workspace("knots-installed-workflows-json-install");
    let root = root_ws.path().to_path_buf();
    let source = root.join("bundle.json");
    let json_bundle = render_json_bundle_from_toml(SAMPLE_BUNDLE).expect("json render");
    std::fs::write(&source, &json_bundle).expect("json bundle should write");

    let workflow_id = install_bundle(&root, &source).expect("json bundle should install");
    assert_eq!(workflow_id, "custom_flow");
    let installed =
        std::fs::read_to_string(workflows_root(&root).join("custom_flow/3/bundle.json"))
            .expect("installed json should read");
    assert_eq!(installed, json_bundle);
}

#[test]
fn falls_back_to_first_profile_without_default() {
    let root_ws = unique_workspace("knots-installed-workflows-first-profile");
    let root = root_ws.path().to_path_buf();
    let source = root.join("bundle.toml");
    let bundle = SAMPLE_BUNDLE
        .replace("default_profile = \"autopilot\"\n", "")
        .replace(
            "[profiles.autopilot]\n\
             description = \"Custom profile\"\n\
             phases = [\"main\"]\n",
            "[profiles.beta]\n\
             description = \"Beta\"\n\
             phases = [\"main\"]\n\n\
             [profiles.alpha]\n\
             description = \"Alpha\"\n\
             phases = [\"main\"]\n",
        );
    std::fs::write(&source, bundle).expect("bundle should write");
    install_bundle(&root, &source).expect("bundle should install");

    let config =
        set_current_workflow_selection(&root, "custom_flow", Some(3), None).expect("select");
    assert_eq!(
        config.default_profile_id_for_workflow("custom_flow"),
        Some("custom_flow/alpha")
    );
}

#[test]
fn loom_failures_and_invalid_utf8_reported() {
    let root_ws = unique_workspace("knots-installed-workflows-loom-errors");
    let root = root_ws.path().to_path_buf();
    let bin_dir = root.join("bin");
    let package_dir = root.join("pkg");
    std::fs::create_dir_all(&bin_dir).expect("bin dir should exist");
    std::fs::create_dir_all(&package_dir).expect("pkg dir should exist");
    std::fs::write(package_dir.join("loom.toml"), "name = 'pkg'").expect("loom.toml writes");

    let loom_path = bin_dir.join("loom");
    // Point the builder directly at the fake binary's absolute path instead
    // of mutating process-global PATH; see read_bundle_source_can_shell_out_to_loom.
    let builder = CommandLoomBundleBuilder::with_binary(&loom_path);

    write_loom_failure_script(&loom_path);
    let err = read_bundle_source_with_builder(&package_dir, &builder)
        .expect_err("loom failure should bubble up");
    assert!(err
        .to_string()
        .contains("loom build --emit knots-bundle failed"));

    write_loom_invalid_utf8_script(&loom_path);
    let err = read_bundle_source_with_builder(&package_dir, &builder)
        .expect_err("invalid utf8 should fail");
    assert!(err.to_string().contains("invalid UTF-8 bundle output"));
}

#[test]
fn builtin_workflow_renders_builtin_prompt_variants_per_profile() {
    let root_ws = unique_workspace("knots-installed-workflows-compat-prompts");
    let root = root_ws.path().to_path_buf();
    ensure_builtin_registry(&root);
    let registry = InstalledWorkflowRegistry::load(&root).expect("registry should load");
    let workflow = registry
        .require_workflow(&builtin_workflow_id_for_knot_type(
            crate::domain::knot_type::KnotType::Work,
        ))
        .expect("builtin workflow should exist");
    let branch_profile = workflow
        .require_profile("autopilot")
        .expect("autopilot should exist");
    let pr_profile = workflow
        .require_profile("autopilot_with_pr")
        .expect("autopilot_with_pr should exist");

    let branch_prompt = branch_profile
        .prompt_for_action_state("implementation")
        .expect("branch prompt should render");
    assert!(branch_prompt.contains("branch itself is the review artifact"));
    assert!(branch_prompt.contains("feature branch pushed to remote"));

    let pr_prompt = pr_profile
        .prompt_for_action_state("implementation")
        .expect("pr prompt should render");
    assert!(pr_prompt.contains("open a pull request from the feature"));
    assert!(pr_prompt.contains("pull request opened from the feature branch"));
}

#[test]
fn profile_registry_load_for_repo_merges_builtin_prompt_variants() {
    let root_ws = unique_workspace("knots-installed-workflows-compat-registry");
    let root = root_ws.path().to_path_buf();
    ensure_builtin_registry(&root);
    let registry = crate::profile::ProfileRegistry::load_for_repo(&root)
        .expect("profile registry should load");
    let branch_profile = registry
        .require("autopilot")
        .expect("autopilot should exist");
    let pr_profile = registry
        .require("autopilot_with_pr")
        .expect("autopilot_with_pr should exist");

    let branch_prompt = branch_profile
        .prompt_for_action_state("shipment")
        .expect("branch shipment prompt should exist");
    assert!(branch_prompt.contains("merge the feature branch to main"));
    assert!(!branch_prompt.contains("merge the approved pull request"));

    let pr_prompt = pr_profile
        .prompt_for_action_state("shipment")
        .expect("pr shipment prompt should exist");
    assert!(pr_prompt.contains("merge the approved pull request"));
    assert!(!pr_prompt.contains("merge the feature branch to main"));
}

#[test]
fn registry_rejects_knot_types_without_any_resolvable_workflows() {
    let root_ws = unique_workspace("knots-installed-workflows-empty-knot-type");
    let root = root_ws.path().to_path_buf();
    let mut config = ensure_builtin_registry(&root);
    config.knot_type_workflows.insert(
        "explore".to_string(),
        KnotTypeWorkflowConfig {
            default: WorkflowRef::new("", None),
            registered: Vec::new(),
        },
    );
    write_repo_config(&root, &config).expect("config writes");

    let err = InstalledWorkflowRegistry::load(&root).expect_err("load should fail");
    assert!(err
        .to_string()
        .contains("knot type 'explore' has no registered workflows"));
}

#[test]
fn registry_rejects_missing_default_workflow_even_if_registered_entries_exist() {
    let root_ws = unique_workspace("knots-installed-workflows-missing-default");
    let root = root_ws.path().to_path_buf();
    let mut config = ensure_builtin_registry(&root);
    config.knot_type_workflows.insert(
        "explore".to_string(),
        KnotTypeWorkflowConfig {
            default: WorkflowRef::new("missing_flow", Some(7)),
            registered: vec![WorkflowRef::new("explore_sdlc", Some(1))],
        },
    );
    write_repo_config(&root, &config).expect("config writes");

    let err = InstalledWorkflowRegistry::load(&root).expect_err("load should fail");
    assert!(
        matches!(err, crate::profile::ProfileError::UnknownWorkflow(id) if id == "missing_flow")
    );
}

#[test]
fn ensure_builtin_registration_adds_missing_entries_without_changing_defaults() {
    let root_ws = unique_workspace("knots-installed-workflows-register-builtins");
    let root = root_ws.path().to_path_buf();
    let source = root.join("custom-flow.toml");
    std::fs::write(&source, SAMPLE_BUNDLE).expect("bundle should write");
    install_bundle(&root, &source).expect("bundle should install");

    let mut config = ensure_builtin_registry(&root);
    config.knot_type_workflows.insert(
        KnotType::Work.as_str().to_string(),
        KnotTypeWorkflowConfig {
            default: WorkflowRef::new("custom_flow", Some(3)),
            registered: vec![WorkflowRef::new("custom_flow", Some(3))],
        },
    );
    config.knot_type_workflows.insert(
        KnotType::Explore.as_str().to_string(),
        KnotTypeWorkflowConfig {
            default: WorkflowRef::new("custom_flow", Some(3)),
            registered: vec![WorkflowRef::new("custom_flow", Some(3))],
        },
    );
    write_repo_config(&root, &config).expect("config writes");

    let config = ensure_builtin_workflows_registered(&root).expect("builtin registration");
    assert_eq!(
        config
            .current_workflow_ref_for_knot_type(KnotType::Work)
            .map(|workflow| workflow.workflow_id),
        Some("custom_flow".to_string())
    );
    assert_eq!(
        config
            .current_workflow_ref_for_knot_type(KnotType::Explore)
            .map(|workflow| workflow.workflow_id),
        Some("custom_flow".to_string())
    );
    assert_eq!(
        config
            .current_workflow_ref_for_knot_type(KnotType::Gate)
            .map(|workflow| workflow.workflow_id),
        Some("gate_sdlc".to_string())
    );
    assert_eq!(
        config
            .current_workflow_ref_for_knot_type(KnotType::Lease)
            .map(|workflow| workflow.workflow_id),
        Some("lease_sdlc".to_string())
    );
    assert_eq!(
        config
            .current_workflow_ref_for_knot_type(KnotType::ExecutionPlan)
            .map(|workflow| workflow.workflow_id),
        Some("execution_plan_sdlc".to_string())
    );
    let registry = InstalledWorkflowRegistry::load(&root).expect("registry should load");
    let work_ids = registry
        .registered_workflows_for_knot_type(KnotType::Work)
        .iter()
        .map(|workflow| workflow.id.as_str())
        .collect::<Vec<_>>();
    assert!(work_ids.contains(&"custom_flow"));
    assert!(work_ids.contains(&"work_sdlc"));

    let explore_ids = registry
        .registered_workflows_for_knot_type(KnotType::Explore)
        .iter()
        .map(|workflow| workflow.id.as_str())
        .collect::<Vec<_>>();
    assert!(explore_ids.contains(&"custom_flow"));
    assert!(explore_ids.contains(&"explore_sdlc"));

    let execution_plan_ids = registry
        .registered_workflows_for_knot_type(KnotType::ExecutionPlan)
        .iter()
        .map(|workflow| workflow.id.as_str())
        .collect::<Vec<_>>();
    assert!(execution_plan_ids.contains(&"execution_plan_sdlc"));
}

fn make_executable(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path).expect("metadata").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).expect("permissions");
    }
}

fn write_loom_failure_script(loom_path: &std::path::Path) {
    std::fs::write(loom_path, "#!/bin/sh\necho boom >&2\nexit 1\n").expect("script writes");
    make_executable(loom_path);
}

fn write_loom_invalid_utf8_script(loom_path: &std::path::Path) {
    std::fs::write(
        loom_path,
        "#!/bin/sh\n\
         if [ \"$1\" = \"build\" ]; then\n\
         printf '\\377\\376'\nexit 0\nfi\nexit 1\n",
    )
    .expect("invalid utf8 script writes");
    make_executable(loom_path);
}
