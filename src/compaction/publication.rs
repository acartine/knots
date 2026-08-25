use std::path::Path;

use crate::sync::{GitAdapter, SyncError};

use super::{validate, CompactionManifest, GenerationState, ValidationContext};

#[derive(Debug, Clone, Default)]
pub(crate) struct GenerationPublisher {
    git: GitAdapter,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PublicationTarget<'a> {
    pub repo: &'a Path,
    pub remote: &'a str,
    pub remote_ref: &'a str,
    pub commit: &'a str,
}

impl GenerationPublisher {
    pub(crate) fn new() -> Self {
        Self {
            git: GitAdapter::new(),
        }
    }

    pub(crate) fn stage(
        &self,
        target: PublicationTarget<'_>,
        manifest: &CompactionManifest,
        context: &ValidationContext<'_>,
    ) -> Result<(), SyncError> {
        require_state(manifest, GenerationState::Prepared)?;
        validate_manifest(manifest, context)?;
        self.git.push_ref_with_lease(
            target.repo,
            target.remote,
            target.commit,
            target.remote_ref,
            None,
        )
    }

    pub(crate) fn activate(
        &self,
        target: PublicationTarget<'_>,
        expected_head: Option<&str>,
        manifest: &CompactionManifest,
        context: &ValidationContext<'_>,
    ) -> Result<(), SyncError> {
        require_state(manifest, GenerationState::Active)?;
        validate_manifest(manifest, context)?;
        self.git.push_ref_with_lease(
            target.repo,
            target.remote,
            target.commit,
            target.remote_ref,
            expected_head,
        )
    }
}

fn require_state(
    manifest: &CompactionManifest,
    expected: GenerationState,
) -> Result<(), SyncError> {
    if manifest.state == expected {
        return Ok(());
    }
    Err(SyncError::Compaction {
        message: format!(
            "generation {} is {:?}, expected {:?}",
            manifest.generation_id, manifest.state, expected
        ),
    })
}

fn validate_manifest(
    manifest: &CompactionManifest,
    context: &ValidationContext<'_>,
) -> Result<(), SyncError> {
    validate(manifest, context).map_err(|error| SyncError::Compaction {
        message: error.to_string(),
    })
}

#[cfg(test)]
#[path = "publication_tests.rs"]
mod tests;
