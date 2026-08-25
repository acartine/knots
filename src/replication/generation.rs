use std::path::Path;

use crate::project::StorePaths;
use crate::sync::{GitAdapter, SyncError};

/// Protocol-v2 local deletion is intentionally disabled.
///
/// A selected ref is not proof that provider policy protects the control,
/// archive, canonical, and writer-inbox refs. A later provider integration may
/// validate those facts and update local projection state, but protocol v2
/// retains every legacy event/index path and every raw pack indefinitely.
pub(super) fn fence_local_store(
    _git: &GitAdapter,
    _repo_root: &Path,
    _store_paths: &StorePaths,
    _checkout: &Path,
    _active_head: &str,
) -> Result<(), SyncError> {
    Err(SyncError::Compaction {
        message: "protocol-v2 provider protection is unavailable; local deletion disabled"
            .to_string(),
    })
}

#[cfg(test)]
#[path = "generation_tests.rs"]
mod tests;
