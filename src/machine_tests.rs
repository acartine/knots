use std::path::PathBuf;

use super::*;

fn unique_store_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-machine-test-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("store root should be creatable");
    root
}

#[test]
fn persisted_machine_id_is_stable_across_calls() {
    let root = unique_store_root();

    let first = persisted_machine_id(&root);
    let second = persisted_machine_id(&root);

    assert_eq!(first, second, "machine id must be stable across processes");
    assert_eq!(first.len(), 32);
    assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn persisted_machine_id_writes_the_store_local_file() {
    let root = unique_store_root();

    let id = persisted_machine_id(&root);

    let stored = std::fs::read_to_string(root.join(MACHINE_ID_FILE))
        .expect("machine id file should be written");
    assert_eq!(stored.trim(), id);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn persisted_machine_id_reuses_an_existing_file() {
    let root = unique_store_root();
    std::fs::write(root.join(MACHINE_ID_FILE), "  preexisting-id  \n")
        .expect("machine id file should be writable");

    assert_eq!(persisted_machine_id(&root), "preexisting-id");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn persisted_machine_id_replaces_a_blank_file() {
    let root = unique_store_root();
    std::fs::write(root.join(MACHINE_ID_FILE), "   \n").expect("file should be writable");

    let id = persisted_machine_id(&root);

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
    let root = unique_store_root();
    std::fs::write(root.join(MACHINE_ID_FILE), "file-id\n").expect("file should be writable");

    let overridden = resolve_machine_id(Some("  env-id  ".to_string()), &root);

    assert_eq!(overridden, "env-id");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn blank_or_absent_env_override_falls_back_to_the_file() {
    let root = unique_store_root();
    std::fs::write(root.join(MACHINE_ID_FILE), "file-id\n").expect("file should be writable");

    assert_eq!(
        resolve_machine_id(Some("   ".to_string()), &root),
        "file-id"
    );
    assert_eq!(resolve_machine_id(None, &root), "file-id");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn machine_id_reads_the_process_environment() {
    let root = unique_store_root();
    std::fs::write(root.join(MACHINE_ID_FILE), "file-id\n").expect("file should be writable");

    assert_eq!(machine_id(&root), "file-id");
    std::fs::remove_dir_all(&root).ok();
}
