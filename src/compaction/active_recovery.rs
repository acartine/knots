use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::active::{ActiveCompactionError, ActiveGenerationManifest};

const QUARANTINE: &str = "v2/activation-quarantine";

pub(super) fn restore_unactivated(store_root: &Path) -> Result<(), ActiveCompactionError> {
    let root = store_root.join(QUARANTINE);
    if !root.exists() {
        return Ok(());
    }
    for generation in directories(&root)? {
        for stream in ["events", "index"] {
            let source = generation.join(stream);
            if source.exists() {
                restore_tree(&source, &store_root.join(stream))?;
            }
        }
        remove_empty_tree(&generation)?;
    }
    remove_empty_tree(&root)?;
    Ok(())
}

pub(super) fn finish_activated(
    store_root: &Path,
    manifest: &ActiveGenerationManifest,
) -> Result<(), ActiveCompactionError> {
    let root = store_root.join(QUARANTINE);
    if !root.exists() {
        return Ok(());
    }
    let retained = manifest
        .packs
        .iter()
        .flat_map(|pack| pack.events.iter())
        .map(|event| {
            (
                (event.path.as_str(), event.event_id.as_str()),
                event.content_sha256.as_str(),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut files = Vec::new();
    collect_files(&root, &mut files)?;
    for path in files {
        let bytes = std::fs::read(&path)?;
        let value: Value = serde_json::from_slice(&bytes)?;
        let event_id = value
            .get("event_id")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid(&path, "missing event_id"))?;
        let packed_path = quarantine_event_path(&root, &path)?;
        let digest = format!("{:x}", Sha256::digest(&bytes));
        if retained.get(&(packed_path.as_str(), event_id)).copied() != Some(digest.as_str()) {
            return Err(invalid(&path, "event is absent from the activated pack"));
        }
        std::fs::remove_file(path)?;
    }
    remove_empty_tree(&root)?;
    Ok(())
}

fn quarantine_event_path(root: &Path, path: &Path) -> Result<String, ActiveCompactionError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| invalid(path, "escaped activation quarantine"))?;
    let mut components = relative.components();
    components
        .next()
        .ok_or_else(|| invalid(path, "missing activation generation"))?;
    let event_path = components.as_path();
    if !event_path.starts_with("events") && !event_path.starts_with("index") {
        return Err(invalid(path, "is outside an event stream"));
    }
    Ok(format!(".knots/{}", event_path.to_string_lossy()))
}

fn restore_tree(source: &Path, target: &Path) -> Result<(), ActiveCompactionError> {
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)? {
        let source_path = entry?.path();
        let target_path = target.join(
            source_path
                .file_name()
                .ok_or_else(|| invalid(&source_path, "path has no filename"))?,
        );
        if source_path.is_dir() {
            restore_tree(&source_path, &target_path)?;
        } else if !target_path.exists() {
            std::fs::rename(&source_path, &target_path)?;
        } else if std::fs::read(&source_path)? == std::fs::read(&target_path)? {
            std::fs::remove_file(&source_path)?;
        } else {
            return Err(invalid(&source_path, "conflicts with a newer active event"));
        }
    }
    remove_empty_tree(source)?;
    Ok(())
}

fn directories(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    Ok(std::fs::read_dir(root)?
        .filter_map(|entry| entry.ok().map(|value| value.path()))
        .filter(|path| path.is_dir())
        .collect())
}

fn collect_files(root: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(&path, files)?;
        } else {
            files.push(path);
        }
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

fn invalid(path: &Path, message: &str) -> ActiveCompactionError {
    ActiveCompactionError::Invalid(format!("{}: {message}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarantine_paths_must_name_an_event_stream() {
        let root = Path::new("/store/v2/activation-quarantine");
        let path = root.join("generation/other/event.json");
        assert!(quarantine_event_path(root, &path).is_err());
    }

    #[test]
    fn restore_moves_new_files_and_rejects_conflicts() {
        let workspace = knots_test_support::workspace("active-restore-tree");
        let source = workspace.path().join("source");
        let target = workspace.path().join("target");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("event.json"), b"first").unwrap();
        restore_tree(&source, &target).unwrap();
        assert_eq!(std::fs::read(target.join("event.json")).unwrap(), b"first");

        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("event.json"), b"second").unwrap();
        assert!(restore_tree(&source, &target).is_err());
        assert!(remove_empty_tree(&workspace.path().join("missing")).is_ok());
    }
}
