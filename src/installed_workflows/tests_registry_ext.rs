use std::collections::BTreeMap;

use super::bundle_toml::render_json_bundle_from_toml;
use super::operations::{read_bundle_source, write_repo_config};
use super::tests_helpers::{env_lock, unique_workspace, SAMPLE_BUNDLE};
use super::*;

#[test]
fn read_bundle_source_can_shell_out_to_loom() {
    let _guard = env_lock().lock().expect("env lock");
    let root = unique_workspace("knots-installed-workflows-loom-dir");
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

    let original_path = std::env::var_os("PATH");
    let joined_path = match &original_path {
        Some(path) => {
            let mut paths = vec![bin_dir.clone()];
            paths.extend(std::env::split_paths(path));
            std::env::join_paths(paths).expect("joined path")
        }
        None => std::env::join_paths([bin_dir.clone()]).expect("joined path"),
    };
    std::env::set_var("PATH", joined_path);

    let (raw, format) = read_bundle_source(&package_dir).expect("loom package should build");
    assert!(matches!(format, BundleFormat::Json));
    assert!(raw.contains("\"format\": \"knots-bundle\""));

    match original_path {
        Some(path) => std::env::set_var("PATH", path),
        None => std::env::remove_var("PATH"),
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn registry_prefers_latest_version() {
    let root = unique_workspace("knots-installed-workflows-latest");
    let v3 = root.join("custom-v3.toml");
    let v4 = root.join("custom-v4.toml");
    std::fs::write(&v3, SAMPLE_BUNDLE).expect("v3 writes");
    std::fs::write(&v4, SAMPLE_BUNDLE.replace("version = 3", "version = 4")).expect("v4 writes");
    install_bundle(&root, &v3).expect("v3 installs");
    install_bundle(&root, &v4).expect("v4 installs");
    write_repo_config(
        &root,
        &WorkflowRepoConfig {
            current_workflow: Some("custom_flow".to_string()),
            current_version: None,
            legacy_current_profile: None,
            default_profiles: BTreeMap::new(),
        },
    )
    .expect("config writes");

    let registry = InstalledWorkflowRegistry::load(&root).expect("registry should load");
    let current = registry
        .current_workflow()
        .expect("current workflow resolves");
    assert_eq!(current.version, 4);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn set_selection_honors_explicit_profile() {
    let root = unique_workspace("knots-installed-workflows-explicit-profile");
    let source = root.join("custom-flow.toml");
    std::fs::write(&source, SAMPLE_BUNDLE).expect("bundle should write");
    install_bundle(&root, &source).expect("bundle should install");

    let config = set_current_workflow_selection(&root, "custom_flow", Some(3), Some("autopilot"))
        .expect("selection should succeed");
    assert_eq!(config.current_version, Some(3));
    assert_eq!(config.current_profile_id(), Some("custom_flow/autopilot"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn set_default_profile_updates_repo_mapping() {
    let root = unique_workspace("knots-installed-workflows-set-default-profile");
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
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn set_default_profile_none_preserves_config() {
    let root = unique_workspace("knots-installed-workflows-keep-default-profile");
    let source = root.join("custom-flow.toml");
    std::fs::write(&source, SAMPLE_BUNDLE).expect("bundle should write");
    install_bundle(&root, &source).expect("bundle should install");

    let selected = set_current_workflow_selection(&root, "custom_flow", Some(3), Some("autopilot"))
        .expect("selection should succeed");
    let preserved = set_workflow_default_profile(&root, "custom_flow", None)
        .expect("existing config should be preserved");
    assert_eq!(preserved, selected);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn load_skips_non_version_and_accepts_legacy_toml() {
    let root = unique_workspace("knots-installed-workflows-load-legacy");
    let workflow_root = workflows_root(&root).join("legacy_flow");
    std::fs::create_dir_all(&workflow_root).expect("workflow root should exist");
    std::fs::write(workflow_root.join("README.txt"), "ignore me").expect("file should write");
    std::fs::create_dir_all(workflow_root.join("not-a-version")).expect("dir should exist");
    std::fs::create_dir_all(workflow_root.join("7")).expect("version dir should exist");
    std::fs::write(
        workflow_root.join("7/bundle.toml"),
        SAMPLE_BUNDLE.replace("custom_flow", "legacy_flow"),
    )
    .expect("legacy bundle should write");

    let registry = InstalledWorkflowRegistry::load(&root).expect("registry should load");
    let workflow = registry
        .require_workflow("legacy_flow")
        .expect("should load from bundle.toml");
    assert_eq!(workflow.id, "legacy_flow");
    assert_eq!(workflow.version, 3);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn install_supports_json_input() {
    let root = unique_workspace("knots-installed-workflows-json-install");
    let source = root.join("bundle.json");
    let json_bundle = render_json_bundle_from_toml(SAMPLE_BUNDLE).expect("json render");
    std::fs::write(&source, &json_bundle).expect("json bundle should write");

    let workflow_id = install_bundle(&root, &source).expect("json bundle should install");
    assert_eq!(workflow_id, "custom_flow");
    let installed =
        std::fs::read_to_string(workflows_root(&root).join("custom_flow/3/bundle.json"))
            .expect("installed json should read");
    assert_eq!(installed, json_bundle);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn falls_back_to_first_profile_without_default() {
    let root = unique_workspace("knots-installed-workflows-first-profile");
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
    assert_eq!(config.current_profile_id(), Some("custom_flow/alpha"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn loom_failures_and_invalid_utf8_reported() {
    let _guard = env_lock().lock().expect("env lock");
    let root = unique_workspace("knots-installed-workflows-loom-errors");
    let bin_dir = root.join("bin");
    let package_dir = root.join("pkg");
    std::fs::create_dir_all(&bin_dir).expect("bin dir should exist");
    std::fs::create_dir_all(&package_dir).expect("pkg dir should exist");
    std::fs::write(package_dir.join("loom.toml"), "name = 'pkg'").expect("loom.toml writes");

    let loom_path = bin_dir.join("loom");
    let original_path = std::env::var_os("PATH");
    let joined_path = match &original_path {
        Some(path) => {
            let mut paths = vec![bin_dir.clone()];
            paths.extend(std::env::split_paths(path));
            std::env::join_paths(paths).expect("joined path")
        }
        None => std::env::join_paths([bin_dir.clone()]).expect("joined path"),
    };
    std::env::set_var("PATH", joined_path);

    write_loom_failure_script(&loom_path);
    let err = read_bundle_source(&package_dir).expect_err("loom failure should bubble up");
    assert!(err
        .to_string()
        .contains("loom build --emit knots-bundle failed"));

    write_loom_invalid_utf8_script(&loom_path);
    let err = read_bundle_source(&package_dir).expect_err("invalid utf8 should fail");
    assert!(err.to_string().contains("invalid UTF-8 bundle output"));

    match original_path {
        Some(path) => std::env::set_var("PATH", path),
        None => std::env::remove_var("PATH"),
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn builtin_workflow_renders_builtin_prompt_variants_per_profile() {
    let root = unique_workspace("knots-installed-workflows-compat-prompts");
    let registry = InstalledWorkflowRegistry::load(&root).expect("registry should load");
    let workflow = registry
        .require_workflow(BUILTIN_WORKFLOW_ID)
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

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn profile_registry_load_for_repo_merges_builtin_prompt_variants() {
    let root = unique_workspace("knots-installed-workflows-compat-registry");
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

    let _ = std::fs::remove_dir_all(root);
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
