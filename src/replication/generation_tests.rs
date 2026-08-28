use super::fence_local_store;
use crate::project::StorePaths;
use crate::sync::{GitAdapter, SyncError};

#[test]
fn generation_two_fails_closed_without_an_active_manifest() {
    let repo = knots_test_support::workspace("knots-v2-fail-closed-repo");
    let store = knots_test_support::workspace("knots-v2-fail-closed-store");
    let store_paths = StorePaths {
        root: store.path().join(".knots"),
    };
    let error = fence_local_store(
        &GitAdapter::new(),
        repo.path(),
        &store_paths,
        repo.path(),
        "1111111111111111111111111111111111111111",
    )
    .expect_err("a generation without a manifest must fail closed");
    assert!(matches!(error, SyncError::Compaction { .. }));
    assert!(error.to_string().contains("compaction"));
}

#[test]
fn fail_closed_fence_retains_every_legacy_file_and_raw_pack() {
    let repo = knots_test_support::workspace("knots-v2-retention-repo");
    let store = knots_test_support::workspace("knots-v2-retention-store");
    let store_paths = StorePaths {
        root: store.path().join(".knots"),
    };
    let files = [
        store_paths.root.join("events/day/event.json"),
        store_paths.root.join("index/day/head.json"),
        store_paths.root.join("snapshots/legacy.snapshot.json"),
        store_paths.root.join("v2/packs/pack-fixture.pack"),
    ];
    for file in &files {
        std::fs::create_dir_all(file.parent().expect("fixture parent"))
            .expect("create fixture parent");
        std::fs::write(file, b"retain forever").expect("write retained fixture");
    }

    let _ = fence_local_store(
        &GitAdapter::new(),
        repo.path(),
        &store_paths,
        repo.path(),
        "1111111111111111111111111111111111111111",
    );
    for file in files {
        assert_eq!(
            std::fs::read(file).expect("retained file remains"),
            b"retain forever"
        );
    }
}
