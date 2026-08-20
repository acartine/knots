use super::check_nested_caches_at;
use crate::doctor::DoctorStatus;
use crate::project::StorePaths;

fn unique_workspace() -> knots_test_support::TestWorkspace {
    knots_test_support::workspace("knots-nested-cache")
}

fn touch_file(path: &std::path::Path) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent dir should be creatable");
    }
    std::fs::write(path, b"").expect("file should be writable");
}

fn store_paths_for(root: &std::path::Path) -> StorePaths {
    StorePaths {
        root: root.join(".knots"),
    }
}

#[test]
fn nested_cache_detected_warns_and_lists_path() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let outer_db = root.join(".knots/cache/state.sqlite");
    let nested_db = root.join(".knots/subdir/.knots/cache/state.sqlite");
    touch_file(&outer_db);
    touch_file(&nested_db);

    let check = check_nested_caches_at(&store_paths_for(&root)).expect("check runs");
    assert_eq!(check.status, DoctorStatus::Warn);
    let data = check.data.as_ref().expect("data present");
    let nested = data["nested"].as_array().expect("nested array");
    assert_eq!(nested.len(), 1);
    let nested_dir = root.join(".knots/subdir/.knots");
    assert_eq!(nested[0].as_str().unwrap(), nested_dir.to_string_lossy());
    assert!(check.detail.contains("rm -rf "));
    assert!(check.detail.contains(&nested_dir.display().to_string()));
}

#[test]
fn clean_repo_has_no_warning() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    touch_file(&root.join(".knots/cache/state.sqlite"));

    let check = check_nested_caches_at(&store_paths_for(&root)).expect("check runs");
    assert_eq!(check.status, DoctorStatus::Pass);
    assert_eq!(check.detail, "no nested .knots caches detected");
}

#[test]
fn worktree_internal_knots_not_flagged() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    touch_file(&root.join(".knots/cache/state.sqlite"));
    // _worktree's internal .knots holds only events under index/, not a cache.
    touch_file(&root.join(".knots/_worktree/.knots/index/2026/04/10/0001-idx.knot_head.json"));

    let check = check_nested_caches_at(&store_paths_for(&root)).expect("check runs");
    assert_eq!(
        check.status,
        DoctorStatus::Pass,
        "internal _worktree/.knots (no cache/) must not trip the check; detail={}",
        check.detail
    );
}

#[test]
fn missing_outer_cache_passes_when_store_empty() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    std::fs::create_dir_all(root.join(".knots")).expect("root dir");

    let check = check_nested_caches_at(&store_paths_for(&root)).expect("check runs");
    assert_eq!(check.status, DoctorStatus::Pass);
}

#[test]
fn missing_store_root_passes() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let store = StorePaths {
        root: root.join("nonexistent-store"),
    };
    let check = check_nested_caches_at(&store).expect("check runs");
    assert_eq!(check.status, DoctorStatus::Pass);
}

#[test]
fn multiple_nested_caches_all_listed_sorted() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    touch_file(&root.join(".knots/cache/state.sqlite"));
    let nested_a = root.join(".knots/zzz/.knots");
    let nested_b = root.join(".knots/a/b/.knots");
    touch_file(&nested_a.join("cache/state.sqlite"));
    // Use cache.lock to also exercise that marker.
    touch_file(&nested_b.join("cache/cache.lock"));

    let check = check_nested_caches_at(&store_paths_for(&root)).expect("check runs");
    assert_eq!(check.status, DoctorStatus::Warn);
    let data = check.data.as_ref().expect("data");
    let nested = data["nested"].as_array().expect("nested array");
    assert_eq!(nested.len(), 2);
    // Sorted: a/b comes before zzz alphabetically.
    assert_eq!(nested[0].as_str().unwrap(), nested_b.to_string_lossy());
    assert_eq!(nested[1].as_str().unwrap(), nested_a.to_string_lossy());
    assert!(check
        .detail
        .contains(&format!("rm -rf {}", nested_a.display())));
    assert!(check
        .detail
        .contains(&format!("rm -rf {}", nested_b.display())));
    assert!(check.detail.starts_with("found 2 nested .knots cache(s)"));
}

#[test]
fn nested_cache_with_only_lock_is_flagged() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    touch_file(&root.join(".knots/cache/state.sqlite"));
    let nested = root.join(".knots/sub/.knots");
    touch_file(&nested.join("cache/cache.lock"));

    let check = check_nested_caches_at(&store_paths_for(&root)).expect("check runs");
    assert_eq!(check.status, DoctorStatus::Warn);
    let data = check.data.as_ref().expect("data");
    assert_eq!(data["nested"].as_array().unwrap().len(), 1);
}

#[test]
fn empty_nested_knots_dir_without_cache_is_ignored() {
    // A bare `.knots/` directory without any cache marker is harmless: it
    // is not a cache, so don't flag it.
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    touch_file(&root.join(".knots/cache/state.sqlite"));
    std::fs::create_dir_all(root.join(".knots/sub/.knots")).expect("dir");

    let check = check_nested_caches_at(&store_paths_for(&root)).expect("check runs");
    assert_eq!(check.status, DoctorStatus::Pass);
}
