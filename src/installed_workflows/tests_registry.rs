use std::collections::BTreeMap;

use super::bundle_toml::render_json_bundle_from_toml;
use super::loader::installed_bundle_path;
use super::operations::{
    read_bundle_source, repo_config_path, resolve_bundle_source_path, write_repo_config,
};
use super::tests_helpers::{unique_workspace, SAMPLE_BUNDLE};
use super::*;
use crate::domain::knot_type::KnotType;

fn builtin_workflow_id() -> String {
    builtin_workflow_id_for_knot_type(KnotType::Work)
}

fn ensure_builtin_registry(root: &std::path::Path) -> WorkflowRepoConfig {
    ensure_builtin_workflows_registered(root).expect("builtin workflows should register")
}

fn seed_legacy_cache_db(root: &std::path::Path) {
    let db_path = workflows_root(root)
        .parent()
        .expect("workflow root parent")
        .join("cache")
        .join("state.sqlite");
    std::fs::create_dir_all(db_path.parent().expect("db parent should exist"))
        .expect("db parent should be creatable");
    let conn = rusqlite::Connection::open(&db_path).expect("legacy db should open");
    conn.execute_batch(
        r#"
CREATE TABLE schema_migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    applied_at TEXT NOT NULL
);
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (1, 'baseline_cache_schema_v1', '2026-02-23T00:00:00Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (2, 'reserved_v2', '2026-02-23T00:00:01Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (3, 'knot_field_parity_v1', '2026-02-23T00:00:02Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (4, 'knot_workflow_identity_v1', '2026-02-23T00:00:03Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (5, 'workflow_id_canonicalize_v1', '2026-02-23T00:00:04Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (6, 'workflow_to_profile_v1', '2026-02-23T00:00:05Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (7, 'knot_invariants_v1', '2026-02-23T00:00:06Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (8, 'knot_step_history_v1', '2026-02-23T00:00:07Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (9, 'knot_gate_data_v1', '2026-02-23T00:00:08Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (10, 'knot_lease_data_v1', '2026-02-23T00:00:09Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (11, 'knot_workflow_id_v2', '2026-02-23T00:00:10Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (12, 'knot_acceptance_v1', '2026-02-23T00:00:11Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (13, 'knot_blocked_provenance_v1', '2026-02-23T00:00:12Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (14, 'lease_expiry_v1', '2026-02-23T00:00:13Z');
INSERT INTO schema_migrations (version, name, applied_at)
VALUES (15, 'builtin_workflow_id_knots_sdlc_v1', '2026-02-23T00:00:14Z');

CREATE TABLE meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
INSERT INTO meta (key, value) VALUES ('schema_version', '15');
INSERT INTO meta (key, value) VALUES ('hot_window_days', '7');
INSERT INTO meta (key, value) VALUES ('sync_policy', 'auto');
INSERT INTO meta (key, value) VALUES ('sync_auto_budget_ms', '750');
INSERT INTO meta (key, value) VALUES ('sync_try_lock_ms', '0');
INSERT INTO meta (key, value) VALUES ('push_retry_budget_ms', '800');
INSERT INTO meta (key, value) VALUES ('sync_fetch_blob_limit_kb', '0');
INSERT INTO meta (key, value) VALUES ('pull_drift_warn_threshold', '25');

CREATE TABLE knot_hot (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    state TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    body TEXT,
    description TEXT,
    priority INTEGER,
    knot_type TEXT,
    tags_json TEXT NOT NULL DEFAULT '[]',
    notes_json TEXT NOT NULL DEFAULT '[]',
    handoff_capsules_json TEXT NOT NULL DEFAULT '[]',
    invariants_json TEXT NOT NULL DEFAULT '[]',
    step_history_json TEXT NOT NULL DEFAULT '[]',
    gate_data_json TEXT NOT NULL DEFAULT '{}',
    lease_data_json TEXT NOT NULL DEFAULT '{}',
    lease_id TEXT,
    workflow_id TEXT NOT NULL DEFAULT 'knots_sdlc',
    profile_id TEXT NOT NULL DEFAULT 'autopilot',
    profile_etag TEXT,
    deferred_from_state TEXT,
    acceptance TEXT,
    blocked_from_state TEXT,
    lease_expiry_ts INTEGER NOT NULL DEFAULT 0,
    created_at TEXT
);
INSERT INTO knot_hot (
    id, title, state, updated_at, workflow_id, profile_id
) VALUES (
    'K-legacy', 'Legacy', 'ready_for_planning', '2026-02-23T00:00:15Z',
    'knots_sdlc', 'autopilot'
);
"#,
    )
    .expect("legacy schema fixture should write");
}

#[test]
fn repo_config_round_trips_through_disk() {
    let root = unique_workspace("knots-installed-workflows-config");
    let mut config = WorkflowRepoConfig::default();
    config.register_workflow_for_knot_type(
        KnotType::Work,
        WorkflowRef::new("custom_flow", Some(3)),
        true,
    );
    config.set_default_profile("custom_flow", "custom_flow/autopilot".to_string());
    write_repo_config(&root, &config).expect("config should write");
    let loaded = read_repo_config(&root).expect("config should load");
    assert_eq!(loaded, config.normalize());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn normalize_preserves_explicit_profile_mappings() {
    let mut config = WorkflowRepoConfig::default();
    config.register_workflow_for_knot_type(
        KnotType::Work,
        WorkflowRef::new("custom_flow", Some(3)),
        true,
    );
    config.set_default_profile("custom_flow", "custom_flow/explicit".to_string());
    let normalized = config.normalize();
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
        knot_type_workflows: BTreeMap::new(),
        default_profiles: BTreeMap::from([(
            "custom_flow".to_string(),
            "custom_flow/autopilot".to_string(),
        )]),
    };
    assert_eq!(config.current_profile_id(), None);
    assert_eq!(config.default_profile_id_for_workflow("missing"), None);
}

#[test]
fn read_repo_config_rejects_legacy_current_profile() {
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

    let err = read_repo_config(&root).expect_err("legacy config should fail");
    assert!(err.to_string().contains("requires migration"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn ensure_builtin_workflows_registered_migrates_legacy_workflow_id_and_writes_back() {
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

    let loaded = ensure_builtin_registry(&root);
    assert_eq!(
        loaded
            .current_workflow_ref_for_knot_type(crate::domain::knot_type::KnotType::Work)
            .map(|workflow| workflow.workflow_id),
        Some(builtin_workflow_id())
    );
    assert_eq!(loaded.current_profile_id(), Some("autopilot"));
    assert_eq!(
        loaded.default_profile_id_for_workflow(&builtin_workflow_id()),
        Some("autopilot")
    );

    let repaired = std::fs::read_to_string(&path).expect("repaired config should read");
    assert!(repaired.contains("work_sdlc"));
    assert!(!repaired.contains("compatibility"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn ensure_builtin_workflows_registered_migrates_legacy_cache_db() {
    let root = unique_workspace("knots-installed-workflows-repair-cache-db");
    seed_legacy_cache_db(&root);

    ensure_builtin_registry(&root);

    let db_path = root.join(".knots").join("cache").join("state.sqlite");
    let conn = crate::db::open_connection(db_path.to_str().expect("utf8 db path"))
        .expect("upgraded db should open");
    let workflow_id: String = conn
        .query_row(
            "SELECT workflow_id FROM knot_hot WHERE id = 'K-legacy'",
            [],
            |row| row.get(0),
        )
        .expect("legacy row should be present");
    assert_eq!(workflow_id, "work_sdlc");

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
    assert_eq!(
        current
            .current_workflow_ref_for_knot_type(KnotType::Work)
            .map(|workflow| workflow.workflow_id),
        Some(builtin_workflow_id())
    );
    assert_eq!(current.current_profile_id(), None);
    assert_eq!(current.default_profile_id_for_workflow("custom_flow"), None);

    let registry = InstalledWorkflowRegistry::load(&root).expect("registry should load");
    let workflow = registry
        .require_workflow("custom_flow")
        .expect("installed workflow should resolve");
    assert_eq!(workflow.id, "custom_flow");
    assert_eq!(registry.current_workflow_id(), builtin_workflow_id());
    assert_eq!(registry.current_profile_id(), Some("autopilot".to_string()));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn set_selection_keeps_builtin_unscoped() {
    let root = unique_workspace("knots-installed-workflows-builtin");
    let config = set_current_workflow_selection(&root, &builtin_workflow_id(), Some(1), None)
        .expect("builtin workflow should select");
    assert_eq!(
        config
            .current_workflow_ref_for_knot_type(crate::domain::knot_type::KnotType::Work)
            .map(|workflow| workflow.workflow_id),
        Some(builtin_workflow_id())
    );
    assert_eq!(config.current_profile_id(), Some("autopilot"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn set_default_profile_keeps_builtin_unscoped() {
    let root = unique_workspace("knots-installed-workflows-builtin-default");
    let config = set_workflow_default_profile(&root, &builtin_workflow_id(), Some("semiauto"))
        .expect("builtin default profile should persist");
    assert_eq!(
        config.default_profile_id_for_workflow(&builtin_workflow_id()),
        Some("semiauto")
    );

    let registry = InstalledWorkflowRegistry::load(&root).expect("registry should load");
    assert_eq!(
        registry.default_profile_id_for_workflow(&builtin_workflow_id()),
        Some("semiauto".to_string())
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn set_default_profile_none_returns_existing_config() {
    let root = unique_workspace("knots-installed-workflows-default-profile-none");
    ensure_builtin_registry(&root);
    set_workflow_default_profile(&root, &builtin_workflow_id(), Some("semiauto"))
        .expect("builtin default profile should persist");

    let config = set_workflow_default_profile(&root, &builtin_workflow_id(), None)
        .expect("reading existing default profile should succeed");
    assert_eq!(
        config.default_profile_id_for_workflow(&builtin_workflow_id()),
        Some("semiauto")
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
    assert_eq!(installed_bundle_path(&wf_dir), None);
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
    ensure_builtin_registry(&root);
    assert_eq!(
        InstalledWorkflowRegistry::load(&root)
            .expect("registry should load")
            .current_workflow_id(),
        builtin_workflow_id()
    );

    let source = root.join("custom-flow.toml");
    std::fs::write(&source, SAMPLE_BUNDLE).expect("bundle should write");
    install_bundle(&root, &source).expect("bundle should install");

    let registry = InstalledWorkflowRegistry::load(&root).expect("registry should load");
    assert_eq!(registry.current_workflow_version(), Some(1));
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
    assert_eq!(
        listed,
        vec![
            "custom_flow v3",
            "explore_sdlc v1",
            "gate_sdlc v1",
            "lease_sdlc v1",
            "work_sdlc v1",
        ]
    );

    let workflow = registry
        .require_workflow_version("custom_flow", 3)
        .expect("workflow should exist");
    assert_eq!(workflow.display_description(), None);
    assert_eq!(workflow.list_profiles().len(), 1);
    assert!(workflow.require_profile("missing").is_err());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn registry_exposes_builtin_defaults_for_each_knot_type() {
    let root = unique_workspace("knots-installed-workflows-knot-types");
    ensure_builtin_registry(&root);
    let registry = InstalledWorkflowRegistry::load(&root).expect("registry should load");

    let expected: [(KnotType, &str, &str); 4] = [
        (KnotType::Work, "work_sdlc", "autopilot"),
        (KnotType::Gate, "gate_sdlc", "evaluate"),
        (KnotType::Lease, "lease_sdlc", "lease"),
        (KnotType::Explore, "explore_sdlc", "explore"),
    ];

    for (knot_type, workflow_id, profile_id) in expected {
        assert_eq!(
            registry.current_workflow_id_for_knot_type(knot_type),
            workflow_id
        );
        assert_eq!(
            registry.default_profile_id_for_knot_type(knot_type),
            Some(profile_id.to_string())
        );

        let registered = registry.registered_workflows_for_knot_type(knot_type);
        assert_eq!(registered.len(), 1);
        assert_eq!(registered[0].id, workflow_id);
    }

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
        registry.default_profile_id_for_workflow(&builtin_workflow_id()),
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
