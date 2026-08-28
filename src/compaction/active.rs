use std::collections::{BTreeMap, HashSet};
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{build_pack, EventPack, GitObjectProvider, RawEvent};
use crate::snapshots::{write_snapshots_at_store, SnapshotWriteSummary};
use crate::sync::GitAdapter;
use crate::sync_ref::{
    SyncRefConfig, DIFFINITE_GENERATION_TWO_REMOTE_REF, GENERATION_TWO_REMOTE_REF,
};

const MANIFEST_PATH: &str = ".knots/v2/active-generation.json";

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ActiveGenerationManifest {
    pub(super) protocol_version: u32,
    pub(super) source_ref: String,
    pub(super) source_commit: String,
    pub(super) snapshots: Vec<ActiveFile>,
    pub(super) packs: Vec<EventPack>,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ActiveFile {
    pub(super) path: String,
    pub(super) sha256: String,
    pub(super) bytes: u64,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub(crate) struct ActiveCompactionSummary {
    pub legacy_files: u64,
    pub retained_packs: u64,
    pub compacted_ref: String,
    pub compacted_commit: String,
    pub snapshot: SnapshotWriteSummary,
}

#[derive(Debug)]
pub(crate) enum ActiveCompactionError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Snapshot(crate::snapshots::SnapshotError),
    Sync(crate::sync::SyncError),
    Pack(super::PackError),
    Runtime(super::RuntimeError),
    Invalid(String),
}

impl fmt::Display for ActiveCompactionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "compaction I/O error: {error}"),
            Self::Json(error) => write!(f, "compaction JSON error: {error}"),
            Self::Snapshot(error) => write!(f, "compaction snapshot error: {error}"),
            Self::Sync(error) => write!(f, "compaction sync error: {error}"),
            Self::Pack(error) => write!(f, "compaction pack error: {error}"),
            Self::Runtime(error) => write!(f, "compaction Git object error: {error}"),
            Self::Invalid(message) => write!(f, "compaction rejected: {message}"),
        }
    }
}

impl Error for ActiveCompactionError {}

macro_rules! from_error {
    ($source:ty, $variant:ident) => {
        impl From<$source> for ActiveCompactionError {
            fn from(value: $source) -> Self {
                Self::$variant(value)
            }
        }
    };
}

from_error!(std::io::Error, Io);
from_error!(serde_json::Error, Json);
from_error!(crate::snapshots::SnapshotError, Snapshot);
from_error!(crate::sync::SyncError, Sync);
from_error!(super::PackError, Pack);
from_error!(super::RuntimeError, Runtime);

pub(crate) fn activate_generation(
    conn: &Connection,
    repo_root: &Path,
    store_root: &Path,
    source_commit: &str,
) -> Result<ActiveCompactionSummary, ActiveCompactionError> {
    let config = SyncRefConfig::for_repo(repo_root);
    if !config.is_generation_two() {
        super::active_recovery::restore_unactivated(store_root)?;
    }
    let target_ref = generation_ref(config.remote_ref());
    let git = GitAdapter::new();
    let provider = GitObjectProvider::new(repo_root);
    let source_checkout = store_root.join("_worktree");
    if source_checkout.join(".git").exists()
        && git.rev_parse(&source_checkout, "HEAD")? != source_commit
    {
        return Err(ActiveCompactionError::Invalid(
            "fetched source checkout changed during compaction".to_string(),
        ));
    }
    let remote_source = git
        .remote_ref_oid(repo_root, config.remote(), config.remote_ref())?
        .ok_or_else(|| ActiveCompactionError::Invalid("sync source ref is missing".to_string()))?;
    if remote_source != source_commit {
        return Err(ActiveCompactionError::Invalid(
            "sync source advanced during compaction; retry from the new head".to_string(),
        ));
    }
    let expected = git.remote_ref_oid(repo_root, config.remote(), target_ref)?;
    if config.is_generation_two() {
        validate_active_generation(store_root, &source_checkout)?;
    }
    let source = super::active_source::read_source_generation(&provider, source_commit)?;
    let events = source.events;
    let cleanup = read_active_events(store_root)?;
    verify_cleanup_events(&cleanup, &events)?;
    let legacy_files = cleanup.len() as u64;
    if !config.is_generation_two() && expected.is_some() {
        return super::active_resume::resume_legacy_activation(
            &git,
            repo_root,
            store_root,
            &config,
            target_ref,
            expected.as_deref().expect("checked target"),
            source_commit,
            &events,
            &cleanup,
        );
    }
    let snapshot = write_snapshots_at_store(conn, store_root)?;
    let mut packs = if config.is_generation_two() {
        load_existing_manifest(store_root)?.map_or_else(Vec::new, |item| item.packs)
    } else {
        Vec::new()
    };
    reject_duplicate_events(&events, &packs)?;
    if !events.is_empty() {
        let built = build_pack(&events)?;
        persist_pack(store_root, &built.descriptor.path, &built.compressed)?;
        packs.push(built.descriptor);
    }
    packs.sort_by(|left, right| left.pack_id.cmp(&right.pack_id));
    let snapshots = snapshot_files(store_root, &snapshot)?;
    let manifest = ActiveGenerationManifest {
        protocol_version: 2,
        source_ref: config.remote_ref().to_string(),
        source_commit: source_commit.to_string(),
        snapshots,
        packs,
    };
    persist_manifest(store_root, &manifest)?;
    let mut files = generation_files(store_root, &manifest)?;
    for (path, bytes) in source.retained {
        files.entry(path).or_insert(bytes);
    }
    let parent = config
        .is_generation_two()
        .then_some(expected.as_deref())
        .flatten();
    super::active_publish::publish_generation(super::active_publish::Publication {
        git: &git,
        provider: &provider,
        config: &config,
        repo_root,
        store_root,
        target_ref,
        source_commit,
        expected: expected.as_deref(),
        cleanup: &cleanup,
        events: &events,
        files: &files,
        parent,
        legacy_files,
        retained_packs: manifest.packs.len() as u64,
        snapshot,
    })
}

pub(crate) fn validate_active_generation(
    store_root: &Path,
    checkout: &Path,
) -> Result<(), ActiveCompactionError> {
    let bytes = std::fs::read(checkout.join(MANIFEST_PATH))?;
    let manifest: ActiveGenerationManifest = serde_json::from_slice(&bytes)?;
    if manifest.protocol_version != 2 || manifest.snapshots.len() != 2 {
        return Err(ActiveCompactionError::Invalid(
            "active generation manifest is incomplete".to_string(),
        ));
    }
    for file in &manifest.snapshots {
        validate_file(checkout, file)?;
    }
    for pack in &manifest.packs {
        validate_pack(checkout, pack)?;
        let local_pack = store_root.join(local(&pack.path));
        if !local_pack.exists() {
            persist(local_pack, &std::fs::read(checkout.join(&pack.path))?)?;
        }
    }
    super::active_recovery::finish_activated(store_root, &manifest)?;
    persist_manifest(store_root, &manifest)?;
    Ok(())
}

fn validate_file(root: &Path, file: &ActiveFile) -> Result<(), ActiveCompactionError> {
    let bytes = std::fs::read(root.join(&file.path))?;
    validate_digest(&file.path, &bytes, &file.sha256, file.bytes)
}

fn validate_pack(root: &Path, pack: &EventPack) -> Result<(), ActiveCompactionError> {
    let bytes = std::fs::read(root.join(&pack.path))?;
    validate_digest(&pack.path, &bytes, &pack.sha256, pack.bytes)
}

fn validate_digest(
    path: &str,
    bytes: &[u8],
    expected: &str,
    expected_bytes: u64,
) -> Result<(), ActiveCompactionError> {
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual != expected || bytes.len() as u64 != expected_bytes {
        return Err(ActiveCompactionError::Invalid(format!(
            "active generation object failed validation: {path}"
        )));
    }
    Ok(())
}

fn read_active_events(store_root: &Path) -> Result<Vec<RawEvent>, ActiveCompactionError> {
    let mut paths = Vec::new();
    for stream in ["events", "index"] {
        collect_files(&store_root.join(stream), &mut paths)?;
    }
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let bytes = std::fs::read(&path)?;
            let value: serde_json::Value = serde_json::from_slice(&bytes)?;
            let event_id = value
                .get("event_id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    ActiveCompactionError::Invalid(format!(
                        "event has no event_id: {}",
                        path.display()
                    ))
                })?;
            let relative = path.strip_prefix(store_root).map_err(|_| {
                ActiveCompactionError::Invalid("event escaped store root".to_string())
            })?;
            Ok(RawEvent {
                path: format!(".knots/{}", relative.to_string_lossy()),
                event_id: event_id.to_string(),
                bytes,
            })
        })
        .collect()
}

fn collect_files(root: &Path, target: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(&path, target)?;
        } else if path.extension().and_then(|value| value.to_str()) == Some("json") {
            target.push(path);
        }
    }
    Ok(())
}

fn reject_duplicate_events(
    events: &[RawEvent],
    packs: &[EventPack],
) -> Result<(), ActiveCompactionError> {
    let retained = packs
        .iter()
        .flat_map(|pack| pack.events.iter().map(|event| event.event_id.as_str()))
        .collect::<HashSet<_>>();
    if let Some(event) = events
        .iter()
        .find(|event| retained.contains(event.event_id.as_str()))
    {
        return Err(ActiveCompactionError::Invalid(format!(
            "event {} already exists in a retained pack",
            event.event_id
        )));
    }
    Ok(())
}

fn verify_cleanup_events(
    cleanup: &[RawEvent],
    source: &[RawEvent],
) -> Result<(), ActiveCompactionError> {
    let source = source
        .iter()
        .map(|event| (event.path.as_str(), event.bytes.as_slice()))
        .collect::<BTreeMap<_, _>>();
    if let Some(event) = cleanup
        .iter()
        .find(|event| source.get(event.path.as_str()).copied() != Some(event.bytes.as_slice()))
    {
        return Err(ActiveCompactionError::Invalid(format!(
            "local event differs from fetched source: {}",
            event.path
        )));
    }
    Ok(())
}

fn snapshot_files(
    store_root: &Path,
    summary: &SnapshotWriteSummary,
) -> Result<Vec<ActiveFile>, ActiveCompactionError> {
    [&summary.active_path, &summary.cold_path]
        .into_iter()
        .map(|path| describe_file(store_root, path))
        .collect()
}

fn describe_file(store_root: &Path, path: &Path) -> Result<ActiveFile, ActiveCompactionError> {
    let bytes = std::fs::read(path)?;
    let relative = path
        .strip_prefix(store_root)
        .map_err(|_| ActiveCompactionError::Invalid("snapshot escaped store root".to_string()))?;
    Ok(ActiveFile {
        path: format!(".knots/{}", relative.to_string_lossy()),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        bytes: bytes.len() as u64,
    })
}

fn persist_pack(store_root: &Path, path: &str, bytes: &[u8]) -> std::io::Result<()> {
    persist(store_root.join(path.trim_start_matches(".knots/")), bytes)
}

fn persist_manifest(
    store_root: &Path,
    manifest: &ActiveGenerationManifest,
) -> Result<(), ActiveCompactionError> {
    let bytes = serde_json::to_vec_pretty(manifest)?;
    persist(
        store_root.join(MANIFEST_PATH.trim_start_matches(".knots/")),
        &bytes,
    )?;
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

pub(super) fn load_existing_manifest(
    store_root: &Path,
) -> Result<Option<ActiveGenerationManifest>, ActiveCompactionError> {
    let path = store_root.join(MANIFEST_PATH.trim_start_matches(".knots/"));
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_slice(&std::fs::read(path)?)?))
}

pub(super) fn generation_files(
    store_root: &Path,
    manifest: &ActiveGenerationManifest,
) -> Result<BTreeMap<String, Vec<u8>>, ActiveCompactionError> {
    let mut files = BTreeMap::new();
    files.insert(
        MANIFEST_PATH.to_string(),
        serde_json::to_vec_pretty(manifest)?,
    );
    for snapshot in &manifest.snapshots {
        files.insert(
            snapshot.path.clone(),
            std::fs::read(store_root.join(local(&snapshot.path)))?,
        );
    }
    for pack in &manifest.packs {
        let bytes = std::fs::read(store_root.join(local(&pack.path)))?;
        validate_digest(&pack.path, &bytes, &pack.sha256, pack.bytes)?;
        files.insert(pack.path.clone(), bytes);
    }
    Ok(files)
}

fn generation_ref(current: &str) -> &'static str {
    if current.starts_with("refs/work/") {
        DIFFINITE_GENERATION_TWO_REMOTE_REF
    } else {
        GENERATION_TWO_REMOTE_REF
    }
}

pub(super) fn local(path: &str) -> &str {
    path.trim_start_matches(".knots/")
}

#[cfg(test)]
#[path = "active_tests.rs"]
mod tests;
