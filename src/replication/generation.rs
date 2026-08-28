use std::path::Path;

use crate::project::StorePaths;
use crate::sync::{GitAdapter, SyncError};

pub(super) fn fence_local_store(
    _git: &GitAdapter,
    _repo_root: &Path,
    store_paths: &StorePaths,
    checkout: &Path,
    _active_head: &str,
) -> Result<(), SyncError> {
    crate::compaction::validate_active_generation(&store_paths.root, checkout).map_err(|error| {
        SyncError::Compaction {
            message: error.to_string(),
        }
    })
}

#[cfg(test)]
#[path = "generation_tests.rs"]
mod tests;
