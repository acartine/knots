use super::*;

fn unique_store_root() -> knots_test_support::TestWorkspace {
    knots_test_support::workspace("knots-machine-test")
}

#[test]
fn persisted_machine_id_is_stable_across_calls() {
    let root_ws = unique_store_root();
    let root = root_ws.path().to_path_buf();

    let first = persisted_machine_id(&root).expect("first resolution should succeed");
    let second = persisted_machine_id(&root).expect("second resolution should succeed");

    assert_eq!(first, second, "machine id must be stable across processes");
    assert_eq!(first.len(), 32);
    assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn persisted_machine_id_writes_the_store_local_file() {
    let root_ws = unique_store_root();
    let root = root_ws.path().to_path_buf();

    let id = persisted_machine_id(&root).expect("resolution should succeed");

    let stored = std::fs::read_to_string(root.join(MACHINE_ID_FILE))
        .expect("machine id file should be written");
    assert_eq!(stored.trim(), id);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn persisted_machine_id_reuses_an_existing_file() {
    let root_ws = unique_store_root();
    let root = root_ws.path().to_path_buf();
    std::fs::write(root.join(MACHINE_ID_FILE), "  preexisting-id  \n")
        .expect("machine id file should be writable");

    assert_eq!(
        persisted_machine_id(&root).expect("resolution should succeed"),
        "preexisting-id"
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn persisted_machine_id_never_rewrites_an_existing_file() {
    let root_ws = unique_store_root();
    let root = root_ws.path().to_path_buf();
    let path = root.join(MACHINE_ID_FILE);
    std::fs::write(&path, "  preexisting-id  \n").expect("machine id file should be writable");
    let before = std::fs::metadata(&path)
        .expect("metadata should be readable")
        .modified()
        .expect("mtime should be available");

    persisted_machine_id(&root).expect("resolution should succeed");

    let after = std::fs::metadata(&path)
        .expect("metadata should be readable")
        .modified()
        .expect("mtime should be available");
    assert_eq!(
        before, after,
        "an existing machine id file must not be rewritten"
    );
    let stored = std::fs::read_to_string(&path).expect("file should still be readable");
    assert_eq!(stored, "  preexisting-id  \n");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn persisted_machine_id_replaces_a_blank_file() {
    let root_ws = unique_store_root();
    let root = root_ws.path().to_path_buf();
    std::fs::write(root.join(MACHINE_ID_FILE), "   \n").expect("file should be writable");

    let id = persisted_machine_id(&root).expect("resolution should succeed");

    assert_eq!(id.len(), 32);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn derived_machine_id_is_not_the_raw_system_id() {
    let seed = system_seed();
    let derived = derive_machine_id();
    assert!(!seed.is_empty());
    assert_ne!(derived, seed, "machine id must be an opaque digest");
}

#[test]
fn env_override_wins_over_the_persisted_file() {
    let root_ws = unique_store_root();
    let root = root_ws.path().to_path_buf();
    std::fs::write(root.join(MACHINE_ID_FILE), "file-id\n").expect("file should be writable");

    let overridden = resolve_machine_id(Some("  env-id  ".to_string()), &root)
        .expect("override resolution should succeed");

    assert_eq!(overridden, "env-id");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn blank_or_absent_env_override_falls_back_to_the_file() {
    let root_ws = unique_store_root();
    let root = root_ws.path().to_path_buf();
    std::fs::write(root.join(MACHINE_ID_FILE), "file-id\n").expect("file should be writable");

    assert_eq!(
        resolve_machine_id(Some("   ".to_string()), &root).expect("resolution should succeed"),
        "file-id"
    );
    assert_eq!(
        resolve_machine_id(None, &root).expect("resolution should succeed"),
        "file-id"
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn machine_id_reads_the_process_environment() {
    let root_ws = unique_store_root();
    let root = root_ws.path().to_path_buf();
    std::fs::write(root.join(MACHINE_ID_FILE), "file-id\n").expect("file should be writable");

    // Read (never mutate) the real KNOTS_MACHINE_ID so this test is correct
    // whether or not an operator/CI has exported it (docs/leases.md documents
    // that override), and so it can never race the other tests in this suite
    // that call machine_id()/app.machine_id() concurrently. This proves
    // machine_id() is wired to the process environment through
    // resolve_machine_id, using the same injected-env seam the other tests
    // in this file use directly.
    let env_value = std::env::var(MACHINE_ID_ENV).ok();
    assert_eq!(
        machine_id(&root).expect("resolution should succeed"),
        resolve_machine_id(env_value, &root).expect("resolution should succeed"),
    );
    std::fs::remove_dir_all(&root).ok();
}

/// N threads racing `persisted_machine_id` on the very same fresh store must
/// all converge on one value, and that value must equal what actually landed
/// on disk. This is the concurrency bug this knot fixes: previously a racer
/// that lost a plain `fs::write` race could return a different value than
/// the one persisted, causing every later reader of the file to disagree
/// with what that racer already used (e.g. stamped into a lease).
#[test]
fn persisted_machine_id_converges_under_concurrent_first_use() {
    let root_ws = unique_store_root();
    let root = root_ws.path().to_path_buf();
    let threads = 16;

    let handles: Vec<_> = (0..threads)
        .map(|_| {
            let root = root.clone();
            std::thread::spawn(move || {
                persisted_machine_id(&root).expect("every racer should resolve successfully")
            })
        })
        .collect();

    let results: Vec<String> = handles
        .into_iter()
        .map(|h| h.join().expect("racer thread should not panic"))
        .collect();

    let first = &results[0];
    for (i, result) in results.iter().enumerate() {
        assert_eq!(
            result, first,
            "racer {i} diverged from the persisted winner"
        );
    }

    let stored = std::fs::read_to_string(root.join(MACHINE_ID_FILE))
        .expect("machine id file should be written");
    assert_eq!(stored.trim(), first.as_str());

    std::fs::remove_dir_all(&root).ok();
}

/// A store root that cannot hold the machine id file (its parent is a
/// regular file, not a directory, so `create_dir_all` fails) must surface
/// the failure as an `Err`, not silently proceed with an unpersisted,
/// re-randomized id.
#[test]
fn persist_failure_surfaces_as_an_error() {
    let obstruction_ws = knots_test_support::workspace("knots-machine-obstruction");
    let obstruction = obstruction_ws.path().join("not-a-directory");
    std::fs::write(&obstruction, b"not a directory").expect("obstruction file should be writable");
    // The store root sits *under* a path that is itself a plain file, so
    // `create_dir_all` for the store root can never succeed.
    let root = obstruction.join("store-root");

    let result = persisted_machine_id(&root);

    assert!(
        result.is_err(),
        "persist failure must surface, not be swallowed"
    );
    std::fs::remove_file(&obstruction).ok();
}
