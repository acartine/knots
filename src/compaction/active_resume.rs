use std::path::Path;

use super::active::{
    generation_files, load_existing_manifest, local, ActiveCompactionError,
    ActiveCompactionSummary, ActiveGenerationManifest,
};
use super::{GitObjectProvider, RawEvent};
use crate::snapshots::SnapshotWriteSummary;
use crate::sync::GitAdapter;
use crate::sync_ref::SyncRefConfig;

#[allow(clippy::too_many_arguments)]
pub(super) fn resume_legacy_activation(
    git: &GitAdapter,
    repo_root: &Path,
    store_root: &Path,
    config: &SyncRefConfig,
    target_ref: &str,
    expected: &str,
    source_commit: &str,
    events: &[RawEvent],
    cleanup: &[RawEvent],
) -> Result<ActiveCompactionSummary, ActiveCompactionError> {
    let manifest = load_existing_manifest(store_root)?.ok_or_else(conflicting_generation)?;
    if manifest.source_ref != config.remote_ref() || manifest.source_commit != source_commit {
        return Err(conflicting_generation());
    }
    let mut files = generation_files(store_root, &manifest)?;
    let provider = GitObjectProvider::new(repo_root);
    let source = super::active_source::read_source_generation(&provider, source_commit)?;
    for (path, bytes) in source.retained {
        files.entry(path).or_insert(bytes);
    }
    let commit = provider.write_commit(
        files.clone(),
        None,
        "knots: activate compacted event generation\n",
    )?;
    if commit != expected {
        return Err(conflicting_generation());
    }
    let snapshot = existing_snapshot_summary(store_root, &manifest)?;
    super::active_store::activate_local_store(super::active_store::Activation {
        git,
        repo_root,
        store_root,
        current_ref: config.remote_ref(),
        target_ref,
        commit: &commit,
        cleanup,
        source_events: events,
        files: &files,
    })?;
    Ok(ActiveCompactionSummary {
        legacy_files: cleanup.len() as u64,
        retained_packs: manifest.packs.len() as u64,
        compacted_ref: target_ref.to_string(),
        compacted_commit: commit,
        snapshot,
    })
}

fn existing_snapshot_summary(
    store_root: &Path,
    manifest: &ActiveGenerationManifest,
) -> Result<SnapshotWriteSummary, ActiveCompactionError> {
    let [active, cold] = manifest.snapshots.as_slice() else {
        return Err(ActiveCompactionError::Invalid(
            "prepared generation has incomplete snapshots".to_string(),
        ));
    };
    let active_path = store_root.join(local(&active.path));
    let cold_path = store_root.join(local(&cold.path));
    let active_json: serde_json::Value = serde_json::from_slice(&std::fs::read(&active_path)?)?;
    let cold_json: serde_json::Value = serde_json::from_slice(&std::fs::read(&cold_path)?)?;
    let count = |value: &serde_json::Value, key| {
        value
            .get(key)
            .and_then(|items| items.as_array())
            .map_or(0, |items| items.len() as u64)
    };
    Ok(SnapshotWriteSummary {
        active_path,
        cold_path,
        hot_count: count(&active_json, "hot"),
        warm_count: count(&active_json, "warm"),
        cold_count: count(&cold_json, "cold"),
    })
}

fn conflicting_generation() -> ActiveCompactionError {
    ActiveCompactionError::Invalid("another compacted generation already exists".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incomplete_prepared_generation_cannot_resume() {
        let workspace = knots_test_support::workspace("incomplete-active-generation");
        let manifest = ActiveGenerationManifest {
            protocol_version: 2,
            source_ref: "refs/heads/knots".to_string(),
            source_commit: "source".to_string(),
            snapshots: Vec::new(),
            packs: Vec::new(),
        };
        assert!(existing_snapshot_summary(workspace.path(), &manifest).is_err());
        assert!(conflicting_generation()
            .to_string()
            .contains("already exists"));
    }
}
