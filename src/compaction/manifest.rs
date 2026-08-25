use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) const PROTOCOL_VERSION: u32 = 2;
const GENERATION_PREFIX: &str = "g2-";

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GenerationState {
    Prepared,
    Active,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceCheckpoint {
    pub remote_ref: String,
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

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Retention {
    pub max_full_files: u64,
    pub max_index_files: u64,
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
    pub state: GenerationState,
    pub source: SourceCheckpoint,
    pub snapshots: SnapshotSet,
    pub retention: Retention,
    pub compatibility: Compatibility,
    pub previous_generation: Option<String>,
}

#[derive(Serialize)]
struct CanonicalIdentity<'a> {
    protocol_version: u32,
    source: &'a SourceCheckpoint,
    snapshots: &'a SnapshotSet,
    retention: &'a Retention,
    compatibility: &'a Compatibility,
    previous_generation: &'a Option<String>,
}

impl CompactionManifest {
    pub(crate) fn expected_generation_id(&self) -> String {
        let identity = CanonicalIdentity {
            protocol_version: self.protocol_version,
            source: &self.source,
            snapshots: &self.snapshots,
            retention: &self.retention,
            compatibility: &self.compatibility,
            previous_generation: &self.previous_generation,
        };
        let bytes = serde_json::to_vec(&identity)
            .expect("canonical compaction identity must always serialize");
        format!("{GENERATION_PREFIX}{:x}", Sha256::digest(bytes))
    }

    pub(crate) fn seal(mut self) -> Self {
        self.generation_id = self.expected_generation_id();
        self
    }
}
