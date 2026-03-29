use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::project::{
    canonical_or_original, clear_active_project, config_dir, config_path, create_named_project,
    data_dir, delete_named_project, find_git_root, list_named_projects, load_named_project,
    prompt_for_project_selection_from_io, prompt_for_project_selection_with_io, read_global_config,
    resolve_context, set_active_project, validate_project_id, write_global_config, GlobalConfig,
    NamedProjectRecord, StorePaths,
};

fn temp_home(prefix: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7()));
    fs::create_dir_all(&path).expect("temp home should be creatable");
    path
}

#[test]
fn store_paths_and_config_paths_cover_expected_locations() {
    let home = temp_home("knots-project-paths");
    let data_root = data_dir(Some(&home)).expect("data dir should resolve");
    let config_root = config_dir(Some(&home)).expect("config dir should resolve");
    let config_file = config_path(Some(&home)).expect("config path should resolve");
    let store = StorePaths {
        root: data_root.join("projects/demo"),
    };

    assert!(data_root.ends_with(Path::new("knots")));
    assert_eq!(config_file, config_root.join("config.toml"));
    assert!(store.db_path().ends_with(Path::new("cache/state.sqlite")));
    assert!(store.locks_dir().ends_with(Path::new("locks")));
    assert!(store.queue_dir().ends_with(Path::new("queue")));
    assert!(store
        .repo_lock_path()
        .ends_with(Path::new("locks/repo.lock")));
    assert!(store
        .cache_lock_path()
        .ends_with(Path::new("cache/cache.lock")));
    assert!(store
        .write_queue_worker_lock_path()
        .ends_with(Path::new("locks/write_queue_worker.lock")));
    assert!(store.worktree_path().ends_with(Path::new("_worktree")));

    let _ = fs::remove_dir_all(home);
}

#[test]
fn global_config_round_trips_and_active_project_can_be_cleared() {
    let home = temp_home("knots-project-config");
    let config = GlobalConfig {
        default_profile: Some("autopilot".to_string()),
        default_quick_profile: Some("quick".to_string()),
        active_project: Some("demo".to_string()),
    };
    write_global_config(Some(&home), &config).expect("config should write");
    let loaded = read_global_config(Some(&home)).expect("config should load");
    assert_eq!(loaded.active_project.as_deref(), Some("demo"));

    clear_active_project(Some(&home)).expect("active project should clear");
    let cleared = read_global_config(Some(&home)).expect("config should reload");
    assert_eq!(cleared.active_project, None);

    let _ = fs::remove_dir_all(home);
}

#[test]
fn list_and_load_named_projects_cover_empty_stem_fallback_and_errors() {
    let home = temp_home("knots-project-list");
    let projects_root = config_dir(Some(&home))
        .expect("config dir should resolve")
        .join("projects");
    fs::create_dir_all(&projects_root).expect("projects dir should exist");
    fs::write(projects_root.join("ignore.txt"), "skip").expect("non-toml marker should write");
    fs::write(projects_root.join("alpha.toml"), "id = \"\"\n").expect("alpha should write");

    let listed = list_named_projects(Some(&home)).expect("projects should list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "alpha");

    let loaded = load_named_project(Some(&home), "alpha").expect("alpha should load");
    assert_eq!(loaded.id, "alpha");

    fs::write(projects_root.join("broken.toml"), "{").expect("broken project should write");
    let err = list_named_projects(Some(&home)).expect_err("invalid project file should fail");
    assert!(err.contains("invalid project file"));

    let _ = fs::remove_dir_all(home);
}

#[test]
fn create_delete_and_resolve_context_cover_error_paths() {
    let home = temp_home("knots-project-context");
    let repo = home.join("repo");
    fs::create_dir_all(repo.join(".git")).expect("git dir should exist");
    let nested = repo.join("nested/workspace");
    fs::create_dir_all(&nested).expect("nested workspace should exist");

    let project = create_named_project(Some(&home), "demo", Some(&repo)).expect("create project");
    assert_eq!(
        project.repo_root.as_deref(),
        Some(canonical_or_original(&repo).as_path())
    );
    assert!(create_named_project(Some(&home), "demo", None).is_err());

    set_active_project(Some(&home), "demo").expect("set active project");
    let named = resolve_context(None, None, &home, Some(&home)).expect("named context");
    assert_eq!(named.project_id.as_deref(), Some("demo"));

    clear_active_project(Some(&home)).expect("clear active project");
    let git = resolve_context(None, Some(&repo), &home, Some(&home)).expect("git context");
    assert!(git.project_id.is_none());
    let discovered =
        resolve_context(None, None, &nested, Some(&home)).expect("git discovery context");
    assert!(discovered.project_id.is_none());
    assert_eq!(discovered.repo_root, canonical_or_original(&repo));
    let err = resolve_context(None, None, &home, Some(&home)).expect_err("no repo should fail");
    assert!(err.contains("no active project"));

    delete_named_project(Some(&home), "demo").expect("delete project");
    assert!(load_named_project(Some(&home), "demo").is_err());

    let _ = fs::remove_dir_all(home);
}

#[test]
fn prompt_selection_helper_selects_existing_and_creates_new_projects() {
    let home = temp_home("knots-project-prompt");
    create_named_project(Some(&home), "alpha", None).expect("alpha should be created");
    create_named_project(Some(&home), "beta", None).expect("beta should be created");
    let projects = list_named_projects(Some(&home)).expect("projects should list");

    let mut existing_input = Cursor::new(b"2\n".to_vec());
    let mut existing_output = Vec::new();
    let selected = prompt_for_project_selection_with_io(
        &mut existing_input,
        &mut existing_output,
        Some(&home),
        None,
        &projects,
    )
    .expect("selection should succeed");
    assert_eq!(selected.id, "beta");

    let repo = home.join("workspace");
    fs::create_dir_all(&repo).expect("workspace should exist");
    let mut create_input = Cursor::new(b"n\ngamma\n".to_vec());
    let mut create_output = Vec::new();
    let created = prompt_for_project_selection_with_io(
        &mut create_input,
        &mut create_output,
        Some(&home),
        Some(&repo),
        &projects,
    )
    .expect("project creation should succeed");
    assert_eq!(created.id, "gamma");
    assert_eq!(
        created.repo_root.as_deref(),
        Some(canonical_or_original(&repo).as_path())
    );

    let _ = fs::remove_dir_all(home);
}

#[test]
fn prompt_selection_validates_input_and_non_tty_behavior() {
    let home = temp_home("knots-project-prompt-errors");
    let mut output = Vec::new();
    let projects = vec![NamedProjectRecord {
        id: "alpha".to_string(),
        repo_root: None,
    }];

    let mut invalid_selection = Cursor::new(b"nope\n".to_vec());
    let err = prompt_for_project_selection_with_io(
        &mut invalid_selection,
        &mut output,
        Some(&home),
        None,
        &projects,
    )
    .expect_err("invalid selection should fail");
    assert!(err.contains("invalid selection"));

    let mut out_of_range = Cursor::new(b"9\n".to_vec());
    let err = prompt_for_project_selection_with_io(
        &mut out_of_range,
        &mut Vec::new(),
        Some(&home),
        None,
        &projects,
    )
    .expect_err("out of range selection should fail");
    assert!(err.contains("selection out of range"));

    let err = prompt_for_project_selection_from_io(
        &mut Cursor::new(Vec::new()),
        &mut Vec::new(),
        Some(&home),
        None,
        false,
        true,
        &projects,
    )
    .expect_err("non-tty should fail");
    assert!(err.contains("requires a TTY"));

    let _ = fs::remove_dir_all(home);
}

#[test]
fn prompt_selection_from_io_accepts_tty_streams() {
    let home = temp_home("knots-project-prompt-tty");
    create_named_project(Some(&home), "alpha", None).expect("alpha should be created");
    let projects = list_named_projects(Some(&home)).expect("projects should list");

    let selected = prompt_for_project_selection_from_io(
        &mut Cursor::new(b"1\n".to_vec()),
        &mut Vec::new(),
        Some(&home),
        None,
        true,
        true,
        &projects,
    )
    .expect("tty-backed selection should succeed");
    assert_eq!(selected.id, "alpha");

    let _ = fs::remove_dir_all(home);
}

#[test]
fn project_id_validation_and_git_root_search_cover_edge_cases() {
    let home = temp_home("knots-project-validate");
    let nested = home.join("a/b/c");
    fs::create_dir_all(&nested).expect("nested path should exist");
    fs::create_dir_all(home.join(".git")).expect("git marker should exist");

    validate_project_id("demo_1").expect("underscore id should be valid");
    assert!(validate_project_id("").is_err());
    assert!(validate_project_id("Demo").is_err());

    let git_root = find_git_root(&nested).expect("git root should be found");
    assert_eq!(git_root, canonical_or_original(&home));
    assert!(find_git_root(Path::new("/definitely/not/a/repo")).is_none());

    let _ = fs::remove_dir_all(git_root);
}
