use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use crate::compaction::{
    parse_and_validate, CompactionManifest, GenerationState, SourceFacts, ValidationContext,
};
use crate::project::StorePaths;
use crate::sync::{GitAdapter, SyncError};

const MANIFEST_ROOT: &str = ".knots/compaction/generations";
const EVENT_ROOT: &str = ".knots/events";
const INDEX_ROOT: &str = ".knots/index";

pub(super) fn fence_local_store(
    git: &GitAdapter,
    repo_root: &Path,
    store_paths: &StorePaths,
    checkout: &Path,
    active_head: &str,
) -> Result<(), SyncError> {
    let fence = GenerationFence::load(git, repo_root, checkout, active_head)?;
    fence.prune(git, repo_root, store_paths)
}

struct GenerationFence {
    manifest: CompactionManifest,
    cutoff: BTreeMap<PathBuf, String>,
    retained: HashSet<PathBuf>,
}

impl GenerationFence {
    fn load(
        git: &GitAdapter,
        repo_root: &Path,
        checkout: &Path,
        active_head: &str,
    ) -> Result<Self, SyncError> {
        let (manifest, active_bytes, cold_bytes) = load_active_manifest(checkout)?;
        let cutoff = &manifest.source.cutoff_commit;
        let actual_index = git.try_rev_parse(repo_root, &format!("{cutoff}:{INDEX_ROOT}"))?;
        let actual_events = git.try_rev_parse(repo_root, &format!("{cutoff}:{EVENT_ROOT}"))?;
        let source = SourceFacts {
            cutoff_resolves: git.try_rev_parse(repo_root, cutoff)?.is_some(),
            cutoff_is_ancestor: git.is_ancestor(repo_root, cutoff, active_head)?,
            index_tree: actual_index.as_deref(),
            event_tree: actual_events.as_deref(),
        };
        let predecessor = manifest.previous_generation.as_deref();
        let predecessor_chain: Vec<&str> = predecessor.into_iter().collect();
        let context = ValidationContext {
            active_snapshot: Some(&active_bytes),
            cold_snapshot: Some(&cold_bytes),
            source,
            expected_predecessor: predecessor,
            predecessor_chain: &predecessor_chain,
        };
        let manifest_bytes = manifest_bytes(checkout, &manifest.generation_id)?;
        let manifest = parse_and_validate(&manifest_bytes, &context).map_err(compaction_error)?;
        if manifest.state != GenerationState::Active {
            return Err(compaction_message(
                "active generation manifest is not active",
            ));
        }

        let cutoff = load_cutoff(git, repo_root, &manifest)?;
        let retained = collect_json_paths(checkout, &[EVENT_ROOT, INDEX_ROOT])?;
        Ok(Self {
            manifest,
            cutoff,
            retained,
        })
    }

    fn prune(
        &self,
        git: &GitAdapter,
        repo_root: &Path,
        store_paths: &StorePaths,
    ) -> Result<(), SyncError> {
        let local = collect_json_paths(&store_paths.root, &["events", "index"])?;
        for store_relative in local {
            let remote_relative = Path::new(".knots").join(&store_relative);
            let Some(expected_blob) = self.cutoff.get(&remote_relative) else {
                continue;
            };
            let local_path = store_paths.root.join(&store_relative);
            let actual_blob = git.hash_object_file(repo_root, &local_path)?;
            if actual_blob != *expected_blob {
                return Err(SyncError::FileConflict {
                    path: remote_relative,
                });
            }
            if !self.retained.contains(&remote_relative) {
                std::fs::remove_file(local_path)?;
            }
        }
        write_marker(store_paths, &self.manifest.generation_id)
    }
}

fn load_active_manifest(
    checkout: &Path,
) -> Result<(CompactionManifest, Vec<u8>, Vec<u8>), SyncError> {
    let root = checkout.join(MANIFEST_ROOT);
    let mut active = Vec::new();
    if root.exists() {
        for entry in std::fs::read_dir(root)? {
            let path = entry?.path().join("manifest.json");
            if !path.is_file() {
                continue;
            }
            let bytes = std::fs::read(&path)?;
            let manifest: CompactionManifest = serde_json::from_slice(&bytes).map_err(|error| {
                compaction_message(&format!("invalid manifest {}: {error}", path.display()))
            })?;
            if manifest.state == GenerationState::Active {
                active.push(manifest);
            }
        }
    }
    if active.len() != 1 {
        return Err(compaction_message(&format!(
            "generation-two ref must contain exactly one active manifest, found {}",
            active.len()
        )));
    }
    let manifest = active.pop().expect("one active manifest");
    let active_bytes = read_checkout_path(checkout, &manifest.snapshots.active.path)?;
    let cold_bytes = read_checkout_path(checkout, &manifest.snapshots.cold.path)?;
    Ok((manifest, active_bytes, cold_bytes))
}

fn manifest_bytes(checkout: &Path, generation_id: &str) -> Result<Vec<u8>, SyncError> {
    let relative = Path::new(MANIFEST_ROOT)
        .join(generation_id)
        .join("manifest.json");
    read_checkout_path(checkout, &relative.to_string_lossy())
}

fn read_checkout_path(checkout: &Path, relative: &str) -> Result<Vec<u8>, SyncError> {
    let path = checkout.join(relative);
    std::fs::read(&path)
        .map_err(|error| compaction_message(&format!("failed to read {}: {error}", path.display())))
}

fn load_cutoff(
    git: &GitAdapter,
    repo_root: &Path,
    manifest: &CompactionManifest,
) -> Result<BTreeMap<PathBuf, String>, SyncError> {
    let mut paths = BTreeMap::new();
    for (root, tree) in [
        (EVENT_ROOT, manifest.source.event_tree.as_str()),
        (INDEX_ROOT, manifest.source.index_tree.as_str()),
    ] {
        for (relative, blob) in git.list_tree_blobs(repo_root, tree)? {
            paths.insert(Path::new(root).join(relative), blob);
        }
    }
    Ok(paths)
}

fn collect_json_paths(root: &Path, relative_roots: &[&str]) -> Result<HashSet<PathBuf>, SyncError> {
    let mut files = HashSet::new();
    for relative_root in relative_roots {
        let path = root.join(relative_root);
        if !path.exists() {
            continue;
        }
        let mut stack = vec![path];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(dir)? {
                let path = entry?.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path
                    .extension()
                    .is_some_and(|extension| extension == "json")
                {
                    let relative = path.strip_prefix(root).map_err(|error| {
                        compaction_message(&format!("failed to relativize path: {error}"))
                    })?;
                    files.insert(relative.to_path_buf());
                }
            }
        }
    }
    Ok(files)
}

fn write_marker(store_paths: &StorePaths, generation_id: &str) -> Result<(), SyncError> {
    let directory = store_paths.root.join("compaction/local");
    std::fs::create_dir_all(&directory)?;
    let marker = directory.join("active-generation");
    let temporary = directory.join("active-generation.tmp");
    std::fs::write(&temporary, format!("{generation_id}\n"))?;
    std::fs::rename(temporary, marker)?;
    Ok(())
}

fn compaction_error(error: impl std::fmt::Display) -> SyncError {
    compaction_message(&error.to_string())
}

fn compaction_message(message: &str) -> SyncError {
    SyncError::Compaction {
        message: message.to_string(),
    }
}

#[cfg(test)]
#[path = "generation_tests.rs"]
mod tests;
