use std::collections::BTreeMap;
use std::path::Path;

use super::active::{ActiveCompactionError, ActiveCompactionSummary};
use super::{GitObjectProvider, RawEvent};
use crate::snapshots::SnapshotWriteSummary;
use crate::sync::GitAdapter;
use crate::sync_ref::SyncRefConfig;

pub(super) struct Publication<'a> {
    pub git: &'a GitAdapter,
    pub provider: &'a GitObjectProvider,
    pub config: &'a SyncRefConfig,
    pub repo_root: &'a Path,
    pub store_root: &'a Path,
    pub target_ref: &'a str,
    pub source_commit: &'a str,
    pub expected: Option<&'a str>,
    pub cleanup: &'a [RawEvent],
    pub events: &'a [RawEvent],
    pub files: &'a BTreeMap<String, Vec<u8>>,
    pub parent: Option<&'a str>,
    pub legacy_files: u64,
    pub retained_packs: u64,
    pub snapshot: SnapshotWriteSummary,
}

pub(super) fn publish_generation(
    input: Publication<'_>,
) -> Result<ActiveCompactionSummary, ActiveCompactionError> {
    let commit = input.provider.write_commit(
        input.files.clone(),
        input.parent,
        "knots: activate compacted event generation\n",
    )?;
    let current_source = input.git.remote_ref_oid(
        input.repo_root,
        input.config.remote(),
        input.config.remote_ref(),
    )?;
    if current_source.as_deref() != Some(input.source_commit) {
        return Err(ActiveCompactionError::Invalid(
            "sync source advanced before activation; retry from the new head".to_string(),
        ));
    }
    if input.expected != Some(commit.as_str()) {
        if !input.config.is_generation_two() && input.expected.is_some() {
            return Err(ActiveCompactionError::Invalid(
                "another compacted generation already exists".to_string(),
            ));
        }
        input.git.push_ref_with_lease(
            input.repo_root,
            input.config.remote(),
            &commit,
            input.target_ref,
            input.expected,
        )?;
    }
    let published =
        input
            .git
            .remote_ref_oid(input.repo_root, input.config.remote(), input.target_ref)?;
    if published.as_deref() != Some(commit.as_str()) {
        return Err(ActiveCompactionError::Invalid(
            "remote compacted ref did not resolve to the published commit".to_string(),
        ));
    }
    super::active_store::activate_local_store(super::active_store::Activation {
        git: input.git,
        repo_root: input.repo_root,
        store_root: input.store_root,
        current_ref: input.config.remote_ref(),
        target_ref: input.target_ref,
        commit: &commit,
        cleanup: input.cleanup,
        source_events: input.events,
        files: input.files,
    })?;
    Ok(ActiveCompactionSummary {
        legacy_files: input.legacy_files,
        retained_packs: input.retained_packs,
        compacted_ref: input.target_ref.to_string(),
        compacted_commit: commit,
        snapshot: input.snapshot,
    })
}
