use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) const PROTOCOL_VERSION: u32 = 2;
pub(crate) const CANONICAL_ROOT: &str = ".knots/v2";
pub(crate) const CONTROL_REF: &str = "refs/heads/knots-v2-control";
pub(crate) const ARCHIVE_REF_PREFIX: &str = "refs/heads/knots-v2-archive/";
pub(crate) const INBOX_REF_PREFIX: &str = "refs/heads/knots-v2-inbox/";
const GENERATION_PREFIX: &str = "v2-";
const PACK_PREFIX: &str = "pack-";

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceCheckpoint {
    pub legacy_ref: String,
    pub cutoff_commit: String,
    pub index_tree: String,
    pub event_tree: String,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SnapshotDescriptor {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    pub records: u64,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SnapshotSet {
    pub active: SnapshotDescriptor,
    pub cold: SnapshotDescriptor,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EventIndexEntry {
    pub event_id: String,
    pub content_sha256: String,
    pub offset: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EventPack {
    pub pack_id: String,
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    pub decoded_bytes: u64,
    pub events: Vec<EventIndexEntry>,
}

impl EventPack {
    pub(crate) fn expected_pack_id(&self) -> String {
        format!("{PACK_PREFIX}{}", self.sha256)
    }
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WriterHead {
    pub writer_id: String,
    pub inbox_ref: String,
    pub commit: String,
    pub sequence: u64,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Compatibility {
    pub minimum_reader_protocol: u32,
    pub minimum_writer_protocol: u32,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompactionManifest {
    pub protocol_version: u32,
    pub generation_id: String,
    pub predecessor_generation: Option<String>,
    pub predecessor_control_epoch: u64,
    pub source: SourceCheckpoint,
    pub writer_heads: Vec<WriterHead>,
    pub snapshots: SnapshotSet,
    pub projections: SnapshotDescriptor,
    pub packs: Vec<EventPack>,
    pub compatibility: Compatibility,
}

#[derive(Serialize)]
struct CanonicalIdentity<'a> {
    protocol_version: u32,
    predecessor_generation: &'a Option<String>,
    predecessor_control_epoch: u64,
    source: &'a SourceCheckpoint,
    writer_heads: &'a [WriterHead],
    snapshots: &'a SnapshotSet,
    projections: &'a SnapshotDescriptor,
    packs: &'a [EventPack],
    compatibility: &'a Compatibility,
}

impl CompactionManifest {
    pub(crate) fn expected_generation_id(&self) -> String {
        let identity = CanonicalIdentity {
            protocol_version: self.protocol_version,
            predecessor_generation: &self.predecessor_generation,
            predecessor_control_epoch: self.predecessor_control_epoch,
            source: &self.source,
            writer_heads: &self.writer_heads,
            snapshots: &self.snapshots,
            projections: &self.projections,
            packs: &self.packs,
            compatibility: &self.compatibility,
        };
        let bytes = serde_json::to_vec(&identity)
            .expect("canonical compaction identity must always serialize");
        format!("{GENERATION_PREFIX}{:x}", Sha256::digest(bytes))
    }

    pub(crate) fn seal(mut self) -> Self {
        self.writer_heads.sort();
        self.packs
            .sort_by(|left, right| left.pack_id.cmp(&right.pack_id));
        self.generation_id = self.expected_generation_id();
        self
    }

    pub(crate) fn archive_ref(&self) -> String {
        format!("{ARCHIVE_REF_PREFIX}{}", self.generation_id)
    }
}
