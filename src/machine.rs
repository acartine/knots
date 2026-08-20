//! Stable, opaque identity for the machine that owns a lease.
//!
//! Lease knots replicate like any other knot, so a lease created on one
//! machine can land in another machine's cache. Recording an owner on the
//! lease is what lets local-lease queries tell "held here" from "held there".
//!
//! The id is deliberately opaque: it is a salted digest, never a hostname or
//! username, so it records no user identity the store does not already hold.
//!
//! ## First-use persistence
//!
//! The first resolution on a fresh store derives a candidate id and tries to
//! make it durable via [`persist_first_use`]. That path is atomic: any number
//! of racing processes converge on exactly one persisted value, and every
//! racer's return value matches what actually landed on disk (see that
//! function's doc comment for how). A persist failure is returned as an
//! `Err`, never swallowed, because silently proceeding with an unpersisted
//! id would defeat the whole point of a *stable* machine id.
//!
//! ## Deleting `.knots/machine-id`
//!
//! - On platforms with a real system machine id (`/etc/machine-id` or
//!   `/var/lib/dbus/machine-id`, i.e. most Linux hosts): deleting the
//!   persisted file is recoverable. The next resolution re-derives from the
//!   same stable system source and reproduces the identical id.
//! - On platforms with neither source (macOS, Windows): the seed is a fresh
//!   random UUID generated at resolution time, so deleting the persisted
//!   file loses the identity permanently. The next resolution mints and
//!   persists a new, unrelated id. This is a known, accepted trade-off for
//!   this knot's scope; a recoverable per-store seed for these platforms is
//!   a possible future enhancement, not implemented here.

use std::io;
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
pub fn machine_id(store_root: &Path) -> io::Result<String> {
    resolve_machine_id(std::env::var(MACHINE_ID_ENV).ok(), store_root)
}

/// Resolution with the environment passed in, so tests never have to mutate
/// process-global state.
fn resolve_machine_id(env_value: Option<String>, store_root: &Path) -> io::Result<String> {
    match env_value.as_deref().and_then(non_empty) {
        Some(overridden) => Ok(overridden),
        None => persisted_machine_id(store_root),
    }
}

/// Number of times to retry the create-if-absent link when it loses a race
/// to a stale, blank file rather than to a genuine winner. Bounds the loop
/// so a pathological repeat of that (vanishingly unlikely) event can't spin
/// forever; ordinary operation never needs more than one attempt.
const MAX_REPLACE_BLANK_ATTEMPTS: u32 = 5;

/// Read the store-local id file, deriving and persisting one on first use.
fn persisted_machine_id(store_root: &Path) -> io::Result<String> {
    let path = machine_id_path(store_root);
    if let Some(persisted) = read_non_empty(&path) {
        return Ok(persisted);
    }
    let derived = derive_machine_id();
    persist_first_use(&path, &derived)
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
/// digest is persisted. See the module doc for what happens to platforms
/// with no system source if the persisted file is later deleted.
fn system_seed() -> String {
    for source in SYSTEM_SOURCES {
        if let Some(value) = read_non_empty(Path::new(source)) {
            return value;
        }
    }
    uuid::Uuid::now_v7().to_string()
}

/// Durably persist `derived` as the machine id at `path`, the first time
/// this store sees one, with every concurrent racer converging on a single
/// winning value.
///
/// Two processes can reach this function at once on a fresh store (e.g. a
/// read command and a write command both resolving the machine id before
/// either has written `path`). To make that safe:
///
/// 1. Write the full candidate content to a uniquely-named temp file in the
///    same directory and `sync_all` it, so the content is complete and
///    durable before it is ever exposed at `path`.
/// 2. `hard_link` the temp file onto `path`. `hard_link` is atomic
///    create-if-absent: it fails with `AlreadyExists` if another racer's
///    link already landed, and never partially overwrites an existing file.
///
/// A plain `create_new` directly on `path` was considered and rejected: a
/// racer could observe the freshly created (still-empty) file before the
/// winner finishes writing it, a torn-read window. Building the full
/// content off to the side first and only linking it into place removes
/// that window, so a loser's post-`AlreadyExists` read always sees a
/// complete value (never one torn mid-write) -- either the current
/// winner's, or (rarely) a stale blank file predating this atomic scheme,
/// which is not a race winner and is safe to replace; see
/// [`link_or_converge`].
///
/// The loser does not use its own derived candidate when it finds a real
/// winner: it returns the winner's value instead, which is what makes
/// every racer converge on the exact same id.
fn persist_first_use(path: &Path, derived: &str) -> io::Result<String> {
    let dir = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("machine id path {} has no parent directory", path.display()),
        )
    })?;
    std::fs::create_dir_all(dir)?;

    let tmp_path = dir.join(format!(".{}.tmp.{}", MACHINE_ID_FILE, uuid::Uuid::now_v7()));
    write_new_file(&tmp_path, derived)?;
    let outcome = link_or_converge(&tmp_path, path, derived, MAX_REPLACE_BLANK_ATTEMPTS);
    let _ = std::fs::remove_file(&tmp_path);
    outcome
}

/// Try to atomically create `path` by hard-linking `tmp_path` onto it.
///
/// - Success: this process's candidate is now the persisted id.
/// - `AlreadyExists` and the existing file is non-blank: another racer's
///   link landed first. Converge on its value instead of our own.
/// - `AlreadyExists` and the existing file is blank: not a race winner (the
///   atomic path above never links a blank file into place), just stale
///   garbage predating this scheme, or written directly by something else.
///   Remove it and retry. The blank check happens immediately before the
///   removal, right here, to keep the window in which another racer could
///   legitimately win in between as small as possible.
fn link_or_converge(
    tmp_path: &Path,
    path: &Path,
    derived: &str,
    attempts_left: u32,
) -> io::Result<String> {
    match std::fs::hard_link(tmp_path, path) {
        Ok(()) => Ok(derived.to_string()),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
            if let Some(winner) = read_non_empty(path) {
                return Ok(winner);
            }
            if attempts_left == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "machine id file at {} stayed blank after retries",
                        path.display()
                    ),
                ));
            }
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(err),
            }
            link_or_converge(tmp_path, path, derived, attempts_left - 1)
        }
        Err(err) => Err(err),
    }
}

/// Write `value` to a brand-new file at `path`, failing loudly rather than
/// silently if anything goes wrong, and `fsync`ing before returning so the
/// content is durable and complete by the time callers act on it.
fn write_new_file(path: &Path, value: &str) -> io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(format!("{}\n", value).as_bytes())?;
    file.sync_all()
}

#[cfg(test)]
#[path = "machine_tests.rs"]
mod tests;
