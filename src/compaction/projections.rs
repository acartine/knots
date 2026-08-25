use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{SnapshotDescriptor, CANONICAL_ROOT};

#[derive(Debug, Clone, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CanonicalProjections {
    pub schema_version: u32,
    pub hot: Vec<Value>,
    pub warm: Vec<Value>,
    pub cold: Vec<Value>,
    pub edges: Vec<Value>,
    pub leases: Vec<Value>,
    pub workflows: Vec<Value>,
    pub metadata: Vec<Value>,
    pub conflicts: Vec<Value>,
}

impl CanonicalProjections {
    pub(crate) fn record_count(&self) -> u64 {
        [
            self.hot.len(),
            self.warm.len(),
            self.cold.len(),
            self.edges.len(),
            self.leases.len(),
            self.workflows.len(),
            self.metadata.len(),
            self.conflicts.len(),
        ]
        .into_iter()
        .sum::<usize>() as u64
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum ProjectionError {
    UnsupportedSchema,
    Serialization,
}

impl fmt::Display for ProjectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "protocol-v2 projection error: {self:?}")
    }
}

impl Error for ProjectionError {}

pub(crate) fn canonical_projection_bytes(
    projections: &CanonicalProjections,
) -> Result<Vec<u8>, ProjectionError> {
    if projections.schema_version != 1 {
        return Err(ProjectionError::UnsupportedSchema);
    }
    let mut canonical = projections.clone();
    for values in [
        &mut canonical.hot,
        &mut canonical.warm,
        &mut canonical.cold,
        &mut canonical.edges,
        &mut canonical.leases,
        &mut canonical.workflows,
        &mut canonical.metadata,
        &mut canonical.conflicts,
    ] {
        values.sort_by_key(|value| serde_json::to_vec(value).unwrap_or_default());
    }
    serde_json::to_vec(&canonical).map_err(|_| ProjectionError::Serialization)
}

pub(crate) fn projection_descriptor(bytes: &[u8], records: u64) -> SnapshotDescriptor {
    SnapshotDescriptor {
        path: format!("{CANONICAL_ROOT}/generations/current/state.projections.json"),
        sha256: format!("{:x}", Sha256::digest(bytes)),
        bytes: bytes.len() as u64,
        records,
    }
}

#[cfg(test)]
pub(crate) fn fixture_projection_bytes() -> &'static [u8] {
    br#"{"schema_version":1,"hot":[],"warm":[],"cold":[],"edges":[],"leases":[],"workflows":[],"metadata":[],"conflicts":[]}"#
}
