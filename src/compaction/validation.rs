use std::collections::HashSet;
use std::error::Error;
use std::fmt;
use std::path::{Component, Path};

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{
    CompactionManifest, EventPack, SnapshotDescriptor, SourceCheckpoint, V2RefLayout,
    CANONICAL_ROOT, PROTOCOL_VERSION,
};

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
    pub packs: &'a [(&'a str, &'a [u8])],
    pub source: SourceFacts<'a>,
    pub expected_predecessor: Option<&'a str>,
    pub expected_control_epoch: u64,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum ValidationError {
    InvalidJson(String),
    UnsupportedProtocol(u32),
    UnsupportedCompatibility,
    InvalidGenerationId,
    InvalidObjectId(&'static str),
    InvalidLegacyRef,
    UnresolvedSource,
    TreeMismatch(&'static str),
    CutoffNotAncestor,
    MissingSnapshot(&'static str),
    InvalidV2Path(&'static str),
    InvalidDigest(&'static str),
    LengthMismatch(&'static str),
    InvalidSnapshotJson(&'static str),
    SnapshotSchemaMismatch(&'static str),
    SnapshotCountMismatch(&'static str),
    PredecessorMismatch,
    ControlEpochMismatch,
    InvalidWriterHead,
    DuplicateWriter,
    MissingPack,
    InvalidPackId,
    DuplicatePack,
    InvalidEventIndex,
    DuplicateEvent,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid protocol-v2 manifest: {self:?}")
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
    validate_source(&manifest.source, &context.source)?;
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
    validate_writers(manifest)?;
    validate_packs(manifest, context.packs)?;
    if manifest.predecessor_generation.as_deref() != context.expected_predecessor {
        return Err(ValidationError::PredecessorMismatch);
    }
    if manifest.predecessor_control_epoch != context.expected_control_epoch {
        return Err(ValidationError::ControlEpochMismatch);
    }
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
    Ok(())
}

fn validate_source(
    source: &SourceCheckpoint,
    facts: &SourceFacts<'_>,
) -> Result<(), ValidationError> {
    if source.legacy_ref != V2RefLayout::default().legacy {
        return Err(ValidationError::InvalidLegacyRef);
    }
    for (name, object_id) in [
        ("cutoff_commit", source.cutoff_commit.as_str()),
        ("index_tree", source.index_tree.as_str()),
        ("event_tree", source.event_tree.as_str()),
    ] {
        if !valid_object_id(object_id) {
            return Err(ValidationError::InvalidObjectId(name));
        }
    }
    if !facts.cutoff_resolves {
        return Err(ValidationError::UnresolvedSource);
    }
    if facts.index_tree != Some(source.index_tree.as_str()) {
        return Err(ValidationError::TreeMismatch("index_tree"));
    }
    if facts.event_tree != Some(source.event_tree.as_str()) {
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
    if !valid_v2_path(&descriptor.path, ".snapshot.json") {
        return Err(ValidationError::InvalidV2Path(kind));
    }
    let bytes = bytes.ok_or(ValidationError::MissingSnapshot(kind))?;
    validate_bytes(kind, &descriptor.sha256, descriptor.bytes, bytes)?;
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| ValidationError::InvalidSnapshotJson(kind))?;
    if value.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err(ValidationError::SnapshotSchemaMismatch(kind));
    }
    let mut count = 0u64;
    for field in record_fields {
        count += value
            .get(field)
            .and_then(Value::as_array)
            .ok_or(ValidationError::InvalidSnapshotJson(kind))?
            .len() as u64;
    }
    if count != descriptor.records {
        return Err(ValidationError::SnapshotCountMismatch(kind));
    }
    Ok(())
}

fn validate_writers(manifest: &CompactionManifest) -> Result<(), ValidationError> {
    let layout = V2RefLayout::default();
    let mut writers = HashSet::new();
    for head in &manifest.writer_heads {
        if head.writer_id.is_empty()
            || head.inbox_ref != layout.inbox(&head.writer_id)
            || !valid_object_id(&head.commit)
        {
            return Err(ValidationError::InvalidWriterHead);
        }
        if !writers.insert(&head.writer_id) {
            return Err(ValidationError::DuplicateWriter);
        }
    }
    Ok(())
}

fn validate_packs(
    manifest: &CompactionManifest,
    pack_bytes: &[(&str, &[u8])],
) -> Result<(), ValidationError> {
    let mut pack_ids = HashSet::new();
    let mut event_ids = HashSet::new();
    for pack in &manifest.packs {
        if !pack_ids.insert(&pack.pack_id) {
            return Err(ValidationError::DuplicatePack);
        }
        validate_pack(pack, pack_bytes, &mut event_ids)?;
    }
    Ok(())
}

fn validate_pack(
    pack: &EventPack,
    pack_bytes: &[(&str, &[u8])],
    event_ids: &mut HashSet<String>,
) -> Result<(), ValidationError> {
    if pack.pack_id != pack.expected_pack_id()
        || !valid_v2_path(&pack.path, ".pack")
        || !pack.path.ends_with(&format!("{}.pack", pack.pack_id))
    {
        return Err(ValidationError::InvalidPackId);
    }
    let bytes = pack_bytes
        .iter()
        .find_map(|(id, bytes)| (*id == pack.pack_id).then_some(*bytes))
        .ok_or(ValidationError::MissingPack)?;
    validate_bytes("pack", &pack.sha256, pack.bytes, bytes)?;
    for event in &pack.events {
        if event.event_id.is_empty() || !event_ids.insert(event.event_id.clone()) {
            return Err(ValidationError::DuplicateEvent);
        }
        let end = event
            .offset
            .checked_add(event.bytes)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(ValidationError::InvalidEventIndex)?;
        let start =
            usize::try_from(event.offset).map_err(|_| ValidationError::InvalidEventIndex)?;
        let raw = bytes
            .get(start..end)
            .ok_or(ValidationError::InvalidEventIndex)?;
        if !valid_digest(&event.content_sha256)
            || event.content_sha256 != format!("{:x}", Sha256::digest(raw))
        {
            return Err(ValidationError::InvalidEventIndex);
        }
    }
    Ok(())
}

fn validate_bytes(
    kind: &'static str,
    expected_digest: &str,
    expected_len: u64,
    bytes: &[u8],
) -> Result<(), ValidationError> {
    if expected_len != bytes.len() as u64 {
        return Err(ValidationError::LengthMismatch(kind));
    }
    if !valid_digest(expected_digest) || expected_digest != format!("{:x}", Sha256::digest(bytes)) {
        return Err(ValidationError::InvalidDigest(kind));
    }
    Ok(())
}

fn valid_v2_path(value: &str, suffix: &str) -> bool {
    let path = Path::new(value);
    path.starts_with(CANONICAL_ROOT)
        && value.ends_with(suffix)
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
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
