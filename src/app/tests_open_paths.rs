use std::path::PathBuf;

use super::{App, AppError};

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-app-open-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace creatable");
    root
}

#[test]
fn open_returns_not_initialized_when_knots_dir_missing() {
    let root = unique_workspace();
    assert!(!root.join(".knots").exists());

    let result = App::open(".knots/cache/state.sqlite", root.clone());
    assert!(matches!(result, Err(AppError::NotInitialized)));
    assert!(!root.join(".knots").exists());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn open_succeeds_when_knots_dir_exists() {
    let root = unique_workspace();
    std::fs::create_dir_all(root.join(".knots")).expect("create .knots");

    let result = App::open(".knots/cache/state.sqlite", root.clone());
    assert!(result.is_ok());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn open_with_custom_db_path_skips_init_check() {
    let root = unique_workspace();
    let db_path = root.join("custom/state.sqlite");
    let db_str = db_path.to_str().expect("utf8 path");

    let result = App::open(db_str, root.clone());
    assert!(result.is_ok());

    let _ = std::fs::remove_dir_all(root);
}
