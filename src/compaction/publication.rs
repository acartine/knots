use std::path::Path;

use crate::sync::{GitAdapter, SyncError};

use super::{validate, CompactionManifest, V2RefLayout, ValidatedProtection, ValidationContext};

#[derive(Debug, Clone, Default)]
pub(crate) struct GenerationPublisher {
    git: GitAdapter,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PublicationTarget<'a> {
    pub repo: &'a Path,
    pub remote: &'a str,
    pub commit: &'a str,
}

impl GenerationPublisher {
    pub(crate) fn new() -> Self {
        Self {
            git: GitAdapter::new(),
        }
    }

    pub(crate) fn publish_immutable(
        &self,
        target: PublicationTarget<'_>,
        manifest: &CompactionManifest,
        context: &ValidationContext<'_>,
        protection: &ValidatedProtection,
    ) -> Result<(), SyncError> {
        validate_manifest(manifest, context)?;
        require_identity(protection)?;
        let layout = V2RefLayout::default();
        for remote_ref in [
            layout.canonical(&manifest.generation_id),
            layout.archive(&manifest.generation_id),
        ] {
            self.git.push_ref_with_lease(
                target.repo,
                target.remote,
                target.commit,
                &remote_ref,
                None,
            )?;
        }
        Ok(())
    }

    pub(crate) fn activate_control(
        &self,
        target: PublicationTarget<'_>,
        expected_control_head: Option<&str>,
        protection: &ValidatedProtection,
    ) -> Result<(), SyncError> {
        require_identity(protection)?;
        if protection.control_head() != expected_control_head {
            return Err(compaction_message(
                "provider control head does not match expected control head",
            ));
        }
        self.git.push_ref_with_lease(
            target.repo,
            target.remote,
            target.commit,
            V2RefLayout::default().control,
            expected_control_head,
        )
    }
}

fn require_identity(protection: &ValidatedProtection) -> Result<(), SyncError> {
    if protection.repository_id().is_empty() || protection.integrator_id().is_empty() {
        return Err(compaction_message("provider protection identity is empty"));
    }
    Ok(())
}

fn validate_manifest(
    manifest: &CompactionManifest,
    context: &ValidationContext<'_>,
) -> Result<(), SyncError> {
    validate(manifest, context).map_err(|error| compaction_message(&error.to_string()))
}

fn compaction_message(message: &str) -> SyncError {
    SyncError::Compaction {
        message: message.to_string(),
    }
}

#[cfg(test)]
#[path = "publication_tests.rs"]
mod tests;
