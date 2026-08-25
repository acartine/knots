use std::collections::HashSet;
use std::error::Error;
use std::fmt;
use std::path::{Component, Path};

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{CompactionManifest, SnapshotDescriptor, PROTOCOL_VERSION};

#[derive(Debug, Clone, Copy)]
pub(crate) struct SourceFacts<'a> {
    pub cutoff_resolves: bool,
    pub cutoff_is_ancestor: bool,
    pub index_tree: Option<&'a str>,
    pub event_tree: Option<&'a str>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ValidationContext<'a> {
    pub active_snapshot: Option<&'a [u8]>,
    pub cold_snapshot: Option<&'a [u8]>,
    pub source: SourceFacts<'a>,
    pub expected_predecessor: Option<&'a str>,
    pub predecessor_chain: &'a [&'a str],
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum ValidationError {
    InvalidJson(String),
    UnsupportedProtocol(u32),
    UnsupportedCompatibility,
    InvalidGenerationId,
    InvalidRemoteRef,
    InvalidObjectId(&'static str),
    UnresolvedSource,
    TreeMismatch(&'static str),
    CutoffNotAncestor,
    MissingSnapshot(&'static str),
    InvalidSnapshotPath(&'static str),
    InvalidSnapshotDigest(&'static str),
    SnapshotLengthMismatch(&'static str),
    InvalidSnapshotJson(&'static str),
    SnapshotSchemaMismatch(&'static str),
    SnapshotCountMismatch(&'static str),
    InvalidRetention,
    PredecessorMismatch,
    PredecessorCycle,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid compaction manifest: {self:?}")
    }
}

impl Error for ValidationError {}

pub(crate) fn parse_and_validate(
    bytes: &[u8],
    context: &ValidationContext<'_>,
) -> Result<CompactionManifest, ValidationError> {
    let manifest = serde_json::from_slice(bytes)
        .map_err(|error| ValidationError::InvalidJson(error.to_string()))?;
    validate(&manifest, context)?;
    Ok(manifest)
}

pub(crate) fn validate(
    manifest: &CompactionManifest,
    context: &ValidationContext<'_>,
) -> Result<(), ValidationError> {
    validate_header(manifest)?;
    validate_source(manifest, &context.source)?;
    validate_snapshot(
        "active",
        &manifest.snapshots.active,
        context.active_snapshot,
        &["hot", "warm"],
    )?;
    validate_snapshot(
        "cold",
        &manifest.snapshots.cold,
        context.cold_snapshot,
        &["cold"],
    )?;
    validate_predecessors(manifest, context)?;
    Ok(())
}

fn validate_header(manifest: &CompactionManifest) -> Result<(), ValidationError> {
    if manifest.protocol_version != PROTOCOL_VERSION {
        return Err(ValidationError::UnsupportedProtocol(
            manifest.protocol_version,
        ));
    }
    if manifest.compatibility.minimum_reader_protocol != PROTOCOL_VERSION
        || manifest.compatibility.minimum_writer_protocol != PROTOCOL_VERSION
    {
        return Err(ValidationError::UnsupportedCompatibility);
    }
    if manifest.generation_id != manifest.expected_generation_id() {
        return Err(ValidationError::InvalidGenerationId);
    }
    if !valid_remote_ref(&manifest.source.remote_ref) {
        return Err(ValidationError::InvalidRemoteRef);
    }
    if manifest.retention.max_full_files == 0 || manifest.retention.max_index_files == 0 {
        return Err(ValidationError::InvalidRetention);
    }
    Ok(())
}

fn validate_source(
    manifest: &CompactionManifest,
    facts: &SourceFacts<'_>,
) -> Result<(), ValidationError> {
    for (name, object_id) in [
        ("cutoff_commit", manifest.source.cutoff_commit.as_str()),
        ("index_tree", manifest.source.index_tree.as_str()),
        ("event_tree", manifest.source.event_tree.as_str()),
    ] {
        if !valid_object_id(object_id) {
            return Err(ValidationError::InvalidObjectId(name));
        }
    }
    if !facts.cutoff_resolves {
        return Err(ValidationError::UnresolvedSource);
    }
    if facts.index_tree != Some(manifest.source.index_tree.as_str()) {
        return Err(ValidationError::TreeMismatch("index_tree"));
    }
    if facts.event_tree != Some(manifest.source.event_tree.as_str()) {
        return Err(ValidationError::TreeMismatch("event_tree"));
    }
    if !facts.cutoff_is_ancestor {
        return Err(ValidationError::CutoffNotAncestor);
    }
    Ok(())
}

fn validate_snapshot(
    kind: &'static str,
    descriptor: &SnapshotDescriptor,
    bytes: Option<&[u8]>,
    record_fields: &[&str],
) -> Result<(), ValidationError> {
    if !valid_snapshot_path(&descriptor.path) {
        return Err(ValidationError::InvalidSnapshotPath(kind));
    }
    if !valid_digest(&descriptor.sha256) {
        return Err(ValidationError::InvalidSnapshotDigest(kind));
    }
    let bytes = bytes.ok_or(ValidationError::MissingSnapshot(kind))?;
    if descriptor.bytes != bytes.len() as u64 {
        return Err(ValidationError::SnapshotLengthMismatch(kind));
    }
    let digest = format!("{:x}", Sha256::digest(bytes));
    if descriptor.sha256 != digest {
        return Err(ValidationError::InvalidSnapshotDigest(kind));
    }
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| ValidationError::InvalidSnapshotJson(kind))?;
    validate_snapshot_contents(kind, descriptor, &value, record_fields)
}

fn validate_snapshot_contents(
    kind: &'static str,
    descriptor: &SnapshotDescriptor,
    value: &Value,
    record_fields: &[&str],
) -> Result<(), ValidationError> {
    if value.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err(ValidationError::SnapshotSchemaMismatch(kind));
    }
    let mut count = 0u64;
    for field in record_fields {
        let records = value
            .get(field)
            .and_then(Value::as_array)
            .ok_or(ValidationError::InvalidSnapshotJson(kind))?;
        count += records.len() as u64;
    }
    if count != descriptor.records {
        return Err(ValidationError::SnapshotCountMismatch(kind));
    }
    Ok(())
}

fn validate_predecessors(
    manifest: &CompactionManifest,
    context: &ValidationContext<'_>,
) -> Result<(), ValidationError> {
    if manifest.previous_generation.as_deref() != context.expected_predecessor {
        return Err(ValidationError::PredecessorMismatch);
    }
    let mut seen = HashSet::new();
    seen.insert(manifest.generation_id.as_str());
    for generation in context.predecessor_chain {
        if !seen.insert(*generation) {
            return Err(ValidationError::PredecessorCycle);
        }
    }
    if context.predecessor_chain.first().copied() != context.expected_predecessor {
        return Err(ValidationError::PredecessorMismatch);
    }
    Ok(())
}

fn valid_remote_ref(value: &str) -> bool {
    if !value.starts_with("refs/")
        || value.ends_with(['/', '.'])
        || value.contains("..")
        || value.contains("//")
        || value.contains("@{")
        || value
            .bytes()
            .any(|byte| byte <= b' ' || b"~^:?*[\\".contains(&byte))
    {
        return false;
    }
    value
        .split('/')
        .skip(1)
        .all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with(".lock"))
}

fn valid_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_snapshot_path(value: &str) -> bool {
    let path = Path::new(value);
    let mut components = path.components();
    components.next() == Some(Component::Normal(".knots".as_ref()))
        && components.next() == Some(Component::Normal("snapshots".as_ref()))
        && components.clone().count() == 1
        && components.all(|component| matches!(component, Component::Normal(_)))
        && value.ends_with(".snapshot.json")
}
