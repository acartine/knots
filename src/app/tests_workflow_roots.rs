use std::path::PathBuf;

use super::App;
use crate::project::{DistributionMode, GlobalConfig, ProjectContext, StorePaths};

fn unique_workspace(prefix: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    root
}

fn local_only_app(root: &std::path::Path) -> App {
    let context = ProjectContext {
        project_id: Some("demo".to_string()),
        repo_root: root.to_path_buf(),
        store_paths: StorePaths {
            root: root.to_path_buf(),
        },
        distribution: DistributionMode::LocalOnly,
    };
    let db_path = root.join("cache/state.sqlite");
    App::open_with_context(&context, db_path.to_str().expect("utf8 db path"))
        .expect("local-only app should open")
}

#[test]
fn open_with_context_bootstraps_builtin_workflows_for_local_only_store() {
    let root = unique_workspace("knots-app-local-only");
    let app = local_only_app(&root);

    let workflows_root = crate::installed_workflows::workflows_root(&root);
    assert!(workflows_root.join("current").exists());
    assert_eq!(
        app.default_workflow_id().expect("work default workflow"),
        "work_sdlc"
    );
    assert_eq!(
        app.current_workflow_id_for_knot_type(crate::domain::knot_type::KnotType::Explore)
            .expect("explore default workflow"),
        "explore_sdlc"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn home_override_round_trips_user_config_without_real_home() {
    let root = unique_workspace("knots-app-home-override");
    let home = root.join("home");
    std::fs::create_dir_all(&home).expect("home should be creatable");
    let app = local_only_app(&root).with_home_override(Some(home.clone()));
    let config = GlobalConfig {
        default_profile: Some("autopilot".to_string()),
        default_quick_profile: Some("autopilot_no_planning".to_string()),
        active_project: Some("demo".to_string()),
    };

    app.write_user_config(&config)
        .expect("config write should use override home");
    let loaded = app
        .read_user_config()
        .expect("config read should round-trip");
    assert_eq!(loaded.default_profile.as_deref(), Some("autopilot"));
    assert_eq!(
        loaded.default_quick_profile.as_deref(),
        Some("autopilot_no_planning")
    );
    assert_eq!(loaded.active_project.as_deref(), Some("demo"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn home_override_none_rejects_writes_and_unknown_profiles_do_not_resolve() {
    let root = unique_workspace("knots-app-home-none");
    let app = local_only_app(&root).with_home_override(None);

    let err = app
        .write_user_config(&GlobalConfig::default())
        .expect_err("missing home override should reject writes");
    assert!(err.to_string().contains("unable to resolve $HOME"));
    assert_eq!(
        app.resolve_config_profile(&Some("autopilot".to_string())),
        Some("autopilot".to_string())
    );
    assert_eq!(
        app.resolve_config_profile(&Some("missing".to_string())),
        None
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn home_override_none_reads_default_user_config() {
    let root = unique_workspace("knots-app-home-none-default-read");
    let app = local_only_app(&root).with_home_override(None);

    let loaded = app
        .read_user_config()
        .expect("missing home override should return default config");
    assert!(loaded.default_profile.is_none());
    assert!(loaded.default_quick_profile.is_none());
    assert!(loaded.active_project.is_none());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn require_git_distribution_rejects_local_only_actions() {
    let root = unique_workspace("knots-app-local-only-guard");
    let app = local_only_app(&root);

    let err = app
        .require_git_distribution("pull")
        .expect_err("local-only app should reject git-only actions");
    assert!(err.to_string().contains("local-only"));
    assert!(matches!(
        err,
        crate::app::AppError::UnsupportedDistribution { .. }
    ));

    let _ = std::fs::remove_dir_all(root);
}
