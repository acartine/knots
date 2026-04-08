use std::collections::BTreeMap;

use super::bundle_toml::render_json_bundle_from_toml;
use super::loader::installed_bundle_path;
use super::operations::{
    read_bundle_source, repo_config_path, resolve_bundle_source_path, write_repo_config,
};
use super::tests_helpers::{unique_workspace, SAMPLE_BUNDLE};
use super::*;

#[test]
fn repo_config_round_trips_through_disk() {
    let root = unique_workspace("knots-installed-workflows-config");
    let config = WorkflowRepoConfig {
        current_workflow: Some("custom_flow".to_string()),
        current_version: Some(3),
        legacy_current_profile: None,
        default_profiles: BTreeMap::from([(
            "custom_flow".to_string(),
            "custom_flow/autopilot".to_string(),
        )]),
    };
    write_repo_config(&root, &config).expect("config should write");
    let loaded = read_repo_config(&root).expect("config should load");
    assert_eq!(loaded, config);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn normalize_preserves_explicit_profile_mappings() {
    let config = WorkflowRepoConfig {
        current_workflow: Some("custom_flow".to_string()),
        current_version: Some(3),
        legacy_current_profile: Some("custom_flow/legacy".to_string()),
        default_profiles: BTreeMap::from([(
            "custom_flow".to_string(),
            "custom_flow/explicit".to_string(),
        )]),
    };
    let normalized = config.normalize();
    assert_eq!(normalized.legacy_current_profile, None);
    assert_eq!(
        normalized.current_profile_id(),
        Some("custom_flow/explicit")
    );
    assert_eq!(
        normalized.default_profile_id_for_workflow("custom_flow"),
        Some("custom_flow/explicit")
    );
}

#[test]
fn current_profile_is_none_without_workflow() {
    let config = WorkflowRepoConfig {
        current_workflow: None,
        current_version: None,
        legacy_current_profile: None,
        default_profiles: BTreeMap::from([(
            "custom_flow".to_string(),
            "custom_flow/autopilot".to_string(),
        )]),
    };
    assert_eq!(config.current_profile_id(), None);
    assert_eq!(config.default_profile_id_for_workflow("missing"), None);
}

#[test]
fn read_repo_config_migrates_legacy_current_profile() {
    let root = unique_workspace("knots-installed-workflows-legacy-config");
    let path = repo_config_path(&root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("config dir should exist");
    }
    std::fs::write(
        &path,
        "current_workflow = \"custom_flow\"\n\
         current_version = 3\n\
         current_profile = \"custom_flow/autopilot\"\n",
    )
    .expect("legacy config should write");

    let loaded = read_repo_config(&root).expect("config should load");
    assert_eq!(loaded.current_workflow.as_deref(), Some("custom_flow"));
    assert_eq!(loaded.current_profile_id(), Some("custom_flow/autopilot"));
    assert_eq!(
        loaded.default_profile_id_for_workflow("custom_flow"),
        Some("custom_flow/autopilot")
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn read_repo_config_repairs_legacy_builtin_workflow_id_and_writes_back() {
    let root = unique_workspace("knots-installed-workflows-repair-builtin");
    let path = repo_config_path(&root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("config dir should exist");
    }
    std::fs::write(
        &path,
        "current_workflow = \"compatibility\"\n\
         current_version = 1\n\
         [default_profiles]\n\
         compatibility = \"compatibility/autopilot\"\n",
    )
    .expect("legacy config should write");

    let loaded = read_repo_config(&root).expect("config should load");
    assert_eq!(
        loaded.current_workflow.as_deref(),
        Some(BUILTIN_WORKFLOW_ID)
    );
    assert_eq!(loaded.current_profile_id(), Some("autopilot"));
    assert_eq!(
        loaded.default_profile_id_for_workflow(BUILTIN_WORKFLOW_ID),
        Some("autopilot")
    );

    let repaired = std::fs::read_to_string(&path).expect("repaired config should read");
    assert!(repaired.contains("current_workflow = \"knots_sdlc\""));
    assert!(repaired.contains("knots_sdlc = \"autopilot\""));
    assert!(!repaired.contains("compatibility"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn install_bundle_writes_registry_without_switching() {
    let root = unique_workspace("knots-installed-workflows-install");
    let source = root.join("custom-flow.toml");
    std::fs::write(&source, SAMPLE_BUNDLE).expect("bundle should write");

    let workflow_id = install_bundle(&root, &source).expect("bundle should install");
    assert_eq!(workflow_id, "custom_flow");

    let version_dir = workflows_root(&root).join("custom_flow/3");
    assert!(version_dir.join("bundle.json").exists());
    assert!(version_dir.join("bundle.toml").exists());
    assert!(workflows_root(&root)
        .join("custom_flow/bundle.json")
        .exists());

    let current = read_repo_config(&root).expect("current config should load");
    assert_eq!(current.current_workflow, None);
    assert_eq!(current.current_version, None);
    assert!(current.default_profiles.is_empty());

    let registry = InstalledWorkflowRegistry::load(&root).expect("registry should load");
    let workflow = registry
        .require_workflow("custom_flow")
        .expect("installed workflow should resolve");
    assert_eq!(workflow.id, "custom_flow");
    assert_eq!(registry.current_workflow_id(), BUILTIN_WORKFLOW_ID);
    assert_eq!(registry.current_profile_id(), Some("autopilot".to_string()));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn set_selection_keeps_builtin_unscoped() {
    let root = unique_workspace("knots-installed-workflows-builtin");
    let config = set_current_workflow_selection(&root, BUILTIN_WORKFLOW_ID, Some(1), None)
        .expect("builtin workflow should select");
    assert_eq!(
        config.current_workflow.as_deref(),
        Some(BUILTIN_WORKFLOW_ID)
    );
    assert_eq!(config.current_profile_id(), Some("autopilot"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn set_default_profile_keeps_builtin_unscoped() {
    let root = unique_workspace("knots-installed-workflows-builtin-default");
    let config = set_workflow_default_profile(&root, BUILTIN_WORKFLOW_ID, Some("semiauto"))
        .expect("builtin default profile should persist");
    assert_eq!(
        config.default_profile_id_for_workflow(BUILTIN_WORKFLOW_ID),
        Some("semiauto")
    );

    let registry = InstalledWorkflowRegistry::load(&root).expect("registry should load");
    assert_eq!(
        registry.default_profile_id_for_workflow(BUILTIN_WORKFLOW_ID),
        Some("semiauto".to_string())
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn resolve_source_path_finds_candidates_and_errors() {
    let root = unique_workspace("knots-installed-workflows-resolve");
    let candidate_dir = root.join("bundle-dir");
    std::fs::create_dir_all(candidate_dir.join("dist")).expect("dist should exist");
    std::fs::write(candidate_dir.join("dist/bundle.toml"), SAMPLE_BUNDLE)
        .expect("bundle should write");
    let resolved = resolve_bundle_source_path(&candidate_dir).expect("candidate should resolve");
    assert!(resolved.ends_with("dist/bundle.toml"));

    let missing_dir = root.join("missing");
    std::fs::create_dir_all(&missing_dir).expect("missing dir should exist");
    let err = resolve_bundle_source_path(&missing_dir).expect_err("empty dir should fail");
    assert!(err.to_string().contains("no Loom bundle found"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn installed_bundle_path_prefers_json() {
    let root = unique_workspace("knots-installed-workflows-installed-path");
    let wf_dir = root.join("custom_flow/3");
    std::fs::create_dir_all(&wf_dir).expect("workflow dir should exist");
    std::fs::write(wf_dir.join("bundle.toml"), SAMPLE_BUNDLE).expect("toml bundle should write");
    assert_eq!(
        installed_bundle_path(&wf_dir),
        Some(wf_dir.join("bundle.toml"))
    );
    std::fs::write(wf_dir.join("bundle.json"), "{}").expect("json bundle should write");
    assert_eq!(
        installed_bundle_path(&wf_dir),
        Some(wf_dir.join("bundle.json"))
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn registry_helpers_cover_lookup_and_sorting() {
    let root = unique_workspace("knots-installed-workflows-registry");
    assert_eq!(
        InstalledWorkflowRegistry::load(&root)
            .expect("registry should load")
            .current_workflow_id(),
        BUILTIN_WORKFLOW_ID
    );

    let source = root.join("custom-flow.toml");
    std::fs::write(&source, SAMPLE_BUNDLE).expect("bundle should write");
    install_bundle(&root, &source).expect("bundle should install");

    let registry = InstalledWorkflowRegistry::load(&root).expect("registry should load");
    assert_eq!(registry.current_workflow_version(), None);
    assert_eq!(registry.current_profile_id(), Some("autopilot".to_string()));
    assert_eq!(
        registry
            .require_workflow("custom_flow")
            .expect("workflow should exist")
            .to_string(),
        "custom_flow v3"
    );
    assert!(registry.require_workflow("missing").is_err());
    assert!(registry
        .require_workflow_version("custom_flow", 99)
        .is_err());

    let listed = registry
        .list()
        .into_iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    assert_eq!(listed, vec!["custom_flow v3", "knots_sdlc v1"]);

    let workflow = registry
        .require_workflow_version("custom_flow", 3)
        .expect("workflow should exist");
    assert_eq!(workflow.display_description(), None);
    assert_eq!(workflow.list_profiles().len(), 1);
    assert!(workflow.require_profile("missing").is_err());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn bundle_defaults_for_custom_workflows() {
    let root = unique_workspace("knots-installed-workflows-default-profiles");
    let source = root.join("custom-flow.toml");
    std::fs::write(&source, SAMPLE_BUNDLE).expect("bundle should write");
    install_bundle(&root, &source).expect("bundle should install");

    let registry = InstalledWorkflowRegistry::load(&root).expect("registry should load");
    assert_eq!(
        registry.default_profile_id_for_workflow("custom_flow"),
        Some("custom_flow/autopilot".to_string())
    );
    assert_eq!(
        registry.default_profile_id_for_workflow(BUILTIN_WORKFLOW_ID),
        Some("autopilot".to_string())
    );
    assert_eq!(registry.default_profile_id_for_workflow("missing"), None);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn read_bundle_source_supports_file_and_dir() {
    let root = unique_workspace("knots-installed-workflows-read-source");
    let toml_path = root.join("bundle.toml");
    std::fs::write(&toml_path, SAMPLE_BUNDLE).expect("bundle should write");
    let (raw, format) = read_bundle_source(&toml_path).expect("toml bundle should load");
    assert_eq!(raw, SAMPLE_BUNDLE);
    assert!(matches!(format, BundleFormat::Toml));

    let json_dir = root.join("json");
    std::fs::create_dir_all(&json_dir).expect("dir should exist");
    let json_bundle = render_json_bundle_from_toml(SAMPLE_BUNDLE).expect("json render");
    std::fs::write(json_dir.join("bundle.json"), &json_bundle).expect("json bundle writes");
    let (raw, format) = read_bundle_source(&json_dir).expect("json dir should load");
    assert_eq!(raw, json_bundle);
    assert!(matches!(format, BundleFormat::Json));

    let err = read_bundle_source(&root.join("does-not-exist")).expect_err("missing source");
    assert!(err.to_string().contains("does not exist"));
    let _ = std::fs::remove_dir_all(root);
}
