//! Stable, opaque identity for the machine that owns a lease.
//!
//! Lease knots replicate like any other knot, so a lease created on one
//! machine can land in another machine's cache. Recording an owner on the
//! lease is what lets local-lease queries tell "held here" from "held there".
//!
//! The id is deliberately opaque: it is a salted digest, never a hostname or
//! username, so it records no user identity the store does not already hold.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Environment override. Primary seam for tests and for operators who want to
/// pin an identity explicitly.
const MACHINE_ID_ENV: &str = "KNOTS_MACHINE_ID";

/// Domain separation so the persisted value never equals the raw system id.
const MACHINE_ID_SALT: &str = "knots-machine-id-v1";

/// File name under the store root. Only `index/`, `events/` and `snapshots/`
/// are staged for sync, so this file stays local to the machine.
const MACHINE_ID_FILE: &str = "machine-id";

/// System sources tried in order when deriving a fresh id.
const SYSTEM_SOURCES: [&str; 2] = ["/etc/machine-id", "/var/lib/dbus/machine-id"];

/// Resolve the stable id of the machine that owns leases created in this
/// store. Stable across processes: the derived value is persisted on first
/// use and reused thereafter.
pub fn machine_id(store_root: &Path) -> String {
    resolve_machine_id(std::env::var(MACHINE_ID_ENV).ok(), store_root)
}

/// Resolution with the environment passed in, so tests never have to mutate
/// process-global state.
fn resolve_machine_id(env_value: Option<String>, store_root: &Path) -> String {
    env_value
        .as_deref()
        .and_then(non_empty)
        .unwrap_or_else(|| persisted_machine_id(store_root))
}

/// Read the store-local id file, deriving and persisting one on first use.
fn persisted_machine_id(store_root: &Path) -> String {
    let path = machine_id_path(store_root);
    if let Some(persisted) = read_non_empty(&path) {
        return persisted;
    }
    let derived = derive_machine_id();
    persist(&path, &derived);
    derived
}

fn machine_id_path(store_root: &Path) -> PathBuf {
    store_root.join(MACHINE_ID_FILE)
}

fn read_non_empty(path: &Path) -> Option<String> {
    non_empty(&std::fs::read_to_string(path).ok()?)
}

fn non_empty(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn derive_machine_id() -> String {
    let seed = system_seed();
    let mut hasher = Sha256::new();
    hasher.update(MACHINE_ID_SALT.as_bytes());
    hasher.update(b"\0");
    hasher.update(seed.as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
    hex[..32].to_string()
}

/// First readable system machine id, or a random value when the platform
/// exposes none. A random seed still yields a stable id because the derived
/// digest is persisted.
fn system_seed() -> String {
    for source in SYSTEM_SOURCES {
        if let Some(value) = read_non_empty(Path::new(source)) {
            return value;
        }
    }
    uuid::Uuid::now_v7().to_string()
}

fn persist(path: &Path, value: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, format!("{}\n", value));
}

#[cfg(test)]
#[path = "machine_tests.rs"]
mod tests;
