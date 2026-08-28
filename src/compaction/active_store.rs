use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{ActiveCompactionError, RawEvent};
use crate::sync::GitAdapter;
use crate::sync_ref::write_remote_ref_override;

pub(super) struct Activation<'a> {
    pub git: &'a GitAdapter,
    pub repo_root: &'a Path,
    pub store_root: &'a Path,
    pub current_ref: &'a str,
    pub target_ref: &'a str,
    pub commit: &'a str,
    pub cleanup: &'a [RawEvent],
    pub source_events: &'a [RawEvent],
    pub files: &'a BTreeMap<String, Vec<u8>>,
}

pub(super) fn activate_local_store(input: Activation<'_>) -> Result<(), ActiveCompactionError> {
    let quarantine = input
        .store_root
        .join("v2/activation-quarantine")
        .join(uuid::Uuid::now_v7().to_string());
    std::fs::create_dir_all(&quarantine)?;
    for stream in ["events", "index"] {
        let source = input.store_root.join(stream);
        if source.exists() {
            std::fs::rename(&source, quarantine.join(stream))?;
        }
    }
    if let Err(error) = write_remote_ref_override(input.repo_root, input.target_ref) {
        restore_quarantine(input.store_root, &quarantine)?;
        return Err(error.into());
    }
    if let Err(error) = install_worktree_generation(&input) {
        write_remote_ref_override(input.repo_root, input.current_ref)?;
        restore_quarantine(input.store_root, &quarantine)?;
        return Err(error);
    }
    remove_packed_files(&quarantine, input.cleanup)?;
    remove_empty_tree(&quarantine)?;
    let quarantine_root = input.store_root.join("v2/activation-quarantine");
    if quarantine_root.read_dir()?.next().is_none() {
        std::fs::remove_dir(quarantine_root)?;
    }
    Ok(())
}

fn install_worktree_generation(input: &Activation<'_>) -> Result<(), ActiveCompactionError> {
    let worktree = input.store_root.join("_worktree");
    if !worktree.exists() {
        return Ok(());
    }
    remove_worktree_events(&worktree, input.source_events)?;
    for (path, bytes) in input.files {
        persist(worktree.join(path), bytes)?;
    }
    if !worktree.join(".git").exists() {
        return Ok(());
    }
    let mut changed = input
        .source_events
        .iter()
        .map(|event| PathBuf::from(&event.path))
        .collect::<Vec<_>>();
    changed.extend(input.files.keys().map(PathBuf::from));
    input.git.add_path_bufs(&worktree, &changed)?;
    if input.git.has_staged_path_bufs(&worktree, &changed)? {
        input
            .git
            .commit(&worktree, "knots: install compacted generation")?;
    }
    let local_tree = input.git.rev_parse(&worktree, "HEAD:.knots")?;
    let compacted_tree = input
        .git
        .rev_parse(&worktree, &format!("{}:.knots", input.commit))?;
    if local_tree != compacted_tree {
        return Err(ActiveCompactionError::Invalid(
            "local compaction worktree differs from published generation".to_string(),
        ));
    }
    Ok(())
}

fn persist(path: PathBuf, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("compaction path has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(temporary, path)
}

fn restore_quarantine(store_root: &Path, quarantine: &Path) -> std::io::Result<()> {
    for stream in ["events", "index"] {
        let source = quarantine.join(stream);
        if source.exists() {
            std::fs::rename(source, store_root.join(stream))?;
        }
    }
    Ok(())
}

fn remove_packed_files(root: &Path, events: &[RawEvent]) -> std::io::Result<()> {
    for event in events {
        let relative = event.path.strip_prefix(".knots/").ok_or_else(|| {
            std::io::Error::other("packed event path is outside the active store")
        })?;
        std::fs::remove_file(root.join(relative))?;
    }
    Ok(())
}

fn remove_empty_tree(path: &Path) -> std::io::Result<()> {
    if !path.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(path)? {
        remove_empty_tree(&entry?.path())?;
    }
    std::fs::remove_dir(path)
}

fn remove_worktree_events(worktree: &Path, events: &[RawEvent]) -> std::io::Result<()> {
    for event in events {
        let path = worktree.join(&event.path);
        if path.exists() {
            if std::fs::read(&path)? != event.bytes {
                return Err(std::io::Error::other(format!(
                    "worktree event differs from packed source: {}",
                    event.path
                )));
            }
            std::fs::remove_file(path)?;
        }
    }
    for stream in ["events", "index"] {
        let root = worktree.join(".knots").join(stream);
        if root.exists() {
            remove_empty_tree(&root)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_helpers_fail_closed_and_cover_missing_paths() {
        let root_ws = knots_test_support::workspace("active-store-helpers");
        let root = root_ws.path();
        remove_empty_tree(&root.join("missing")).unwrap();

        let bad = RawEvent {
            path: "outside.json".to_string(),
            event_id: "bad".to_string(),
            bytes: b"bad".to_vec(),
        };
        assert!(remove_packed_files(root, &[bad]).is_err());

        let event = RawEvent {
            path: ".knots/events/different.json".to_string(),
            event_id: "different".to_string(),
            bytes: b"expected".to_vec(),
        };
        let path = root.join(&event.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"changed").unwrap();
        assert!(remove_worktree_events(root, &[event]).is_err());

        let quarantine = root.join("quarantine");
        std::fs::create_dir_all(quarantine.join("events")).unwrap();
        std::fs::write(quarantine.join("events/item"), b"event").unwrap();
        restore_quarantine(root, &quarantine).unwrap();
        assert!(root.join("events/item").exists());
    }

    #[test]
    fn install_without_a_worktree_is_a_noop() {
        let root_ws = knots_test_support::workspace("active-store-no-worktree");
        let root = root_ws.path();
        let git = GitAdapter::new();
        let files = BTreeMap::new();
        let commit = "a".repeat(40);
        let input = Activation {
            git: &git,
            repo_root: root,
            store_root: root,
            current_ref: "refs/heads/knots",
            target_ref: "refs/heads/knots-v2",
            commit: &commit,
            cleanup: &[],
            source_events: &[],
            files: &files,
        };
        install_worktree_generation(&input).unwrap();
    }
}
