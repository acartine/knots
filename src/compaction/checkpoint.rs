use std::collections::HashSet;
use std::error::Error;
use std::fmt;

use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;

use crate::db::{ColdCatalogRecord, KnotCacheRecord, WarmKnotRecord};

use super::{
    parse_and_validate, CompactionManifest, ControlRecord, SourceFacts, ValidatedProtection,
    ValidationContext,
};

#[derive(Debug, Clone)]
pub(crate) struct CheckpointInput<'a> {
    pub control_head: &'a str,
    pub control: &'a [u8],
    pub manifest: &'a [u8],
    pub active_snapshot: &'a [u8],
    pub cold_snapshot: &'a [u8],
    pub projections: &'a [u8],
    pub packs: Vec<(&'a str, &'a [u8])>,
    pub source: SourceFacts<'a>,
    pub expected_predecessor: Option<&'a str>,
    pub expected_previous_control_head: Option<&'a str>,
    pub expected_control_epoch: u64,
    pub protection: &'a ValidatedProtection,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedCheckpoint {
    pub(super) control_head: String,
    pub(super) manifest: CompactionManifest,
    pub(super) control: ControlRecord,
    pub(super) hot: Vec<KnotCacheRecord>,
    pub(super) warm: Vec<WarmKnotRecord>,
    pub(super) cold: Vec<ColdCatalogRecord>,
    pub(super) edges: Vec<EdgeProjection>,
    pub(super) auxiliary: Vec<(String, usize, String)>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct CheckpointSummary {
    pub generation_id: String,
    pub control_epoch: u64,
    pub hot: usize,
    pub warm: usize,
    pub cold: usize,
    pub edges: usize,
    pub quarantined: usize,
}

#[derive(Debug)]
pub(crate) enum CheckpointError {
    Validation(String),
    Control(String),
    Projection(String),
    Database(rusqlite::Error),
    Replay(String),
}

impl fmt::Display for CheckpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "protocol-v2 checkpoint error: {self:?}")
    }
}

impl Error for CheckpointError {}

impl From<rusqlite::Error> for CheckpointError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Database(value)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActiveSnapshot {
    schema_version: u32,
    #[serde(default)]
    #[serde(rename = "written_at")]
    _written_at: Option<String>,
    hot: Vec<KnotCacheRecord>,
    warm: Vec<WarmKnotRecord>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ColdSnapshot {
    schema_version: u32,
    #[serde(default)]
    #[serde(rename = "written_at")]
    _written_at: Option<String>,
    cold: Vec<ColdCatalogRecord>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EdgeProjection {
    pub(super) src: String,
    pub(super) kind: String,
    pub(super) dst: String,
}

pub(crate) fn prepare_checkpoint(
    input: CheckpointInput<'_>,
) -> Result<PreparedCheckpoint, CheckpointError> {
    let control: ControlRecord = serde_json::from_slice(input.control)
        .map_err(|error| CheckpointError::Control(error.to_string()))?;
    let context = ValidationContext {
        active_snapshot: Some(input.active_snapshot),
        cold_snapshot: Some(input.cold_snapshot),
        projections: Some(input.projections),
        packs: &input.packs,
        source: input.source,
        expected_predecessor: input.expected_predecessor,
        expected_control_epoch: input.expected_control_epoch,
    };
    let manifest = parse_and_validate(input.manifest, &context)
        .map_err(|error| CheckpointError::Validation(error.to_string()))?;
    validate_control(&control, &manifest, &input)?;
    let active: ActiveSnapshot = decode_snapshot(input.active_snapshot, "active")?;
    let cold: ColdSnapshot = decode_snapshot(input.cold_snapshot, "cold")?;
    if active.schema_version != 1 || cold.schema_version != 1 {
        return Err(CheckpointError::Projection(
            "unsupported catalog snapshot schema".to_string(),
        ));
    }
    let projections: super::CanonicalProjections = serde_json::from_slice(input.projections)
        .map_err(|error| CheckpointError::Projection(error.to_string()))?;
    validate_projection_equivalence(&active, &cold, &projections)?;
    let edges = decode_values(&projections.edges, "edges")?;
    let auxiliary = auxiliary_rows(&projections)?;
    validate_unique_records(&active, &cold, &edges)?;
    Ok(PreparedCheckpoint {
        control_head: input.control_head.to_string(),
        manifest,
        control,
        hot: active.hot,
        warm: active.warm,
        cold: cold.cold,
        edges,
        auxiliary,
    })
}

pub(crate) fn install_checkpoint<F>(
    conn: &Connection,
    checkpoint: &PreparedCheckpoint,
    locally_leased: &HashSet<String>,
    replay_delta: F,
) -> Result<CheckpointSummary, CheckpointError>
where
    F: FnOnce(&Connection) -> Result<(), String>,
{
    super::checkpoint_store::install(conn, checkpoint, locally_leased, replay_delta)
}

fn validate_control(
    control: &ControlRecord,
    manifest: &CompactionManifest,
    input: &CheckpointInput<'_>,
) -> Result<(), CheckpointError> {
    let valid = input.protection.control_head() == Some(input.control_head)
        && control.schema_version == 1
        && control.epoch == manifest.predecessor_control_epoch.saturating_add(1)
        && control.previous_control_head.as_deref() == input.expected_previous_control_head
        && control.active_generation_id == manifest.generation_id
        && control.archive_ref == manifest.archive_ref()
        && control.acknowledged_writer_heads == manifest.writer_heads
        && control.protection_policy_sha256 == input.protection.policy_sha256();
    if valid {
        Ok(())
    } else {
        Err(CheckpointError::Control(
            "control record is not bound to the protected generation".to_string(),
        ))
    }
}

fn decode_snapshot<T: for<'de> Deserialize<'de>>(
    bytes: &[u8],
    kind: &str,
) -> Result<T, CheckpointError> {
    serde_json::from_slice(bytes)
        .map_err(|error| CheckpointError::Projection(format!("{kind}: {error}")))
}

fn decode_values<T: for<'de> Deserialize<'de>>(
    values: &[Value],
    kind: &str,
) -> Result<Vec<T>, CheckpointError> {
    values
        .iter()
        .cloned()
        .map(|value| {
            serde_json::from_value(value)
                .map_err(|error| CheckpointError::Projection(format!("{kind}: {error}")))
        })
        .collect()
}

fn validate_projection_equivalence(
    active: &ActiveSnapshot,
    cold: &ColdSnapshot,
    projections: &super::CanonicalProjections,
) -> Result<(), CheckpointError> {
    let mut active_hot = values(&active.hot)?;
    let mut active_warm = values(&active.warm)?;
    let mut cold_rows = values(&cold.cold)?;
    active_hot.sort_by_key(canonical_value);
    active_warm.sort_by_key(canonical_value);
    cold_rows.sort_by_key(canonical_value);
    if active_hot != projections.hot
        || active_warm != projections.warm
        || cold_rows != projections.cold
    {
        return Err(CheckpointError::Projection(
            "catalog snapshots and canonical projections differ".to_string(),
        ));
    }
    Ok(())
}

fn values<T: serde::Serialize>(records: &[T]) -> Result<Vec<Value>, CheckpointError> {
    records
        .iter()
        .map(|record| {
            serde_json::to_value(record)
                .map_err(|error| CheckpointError::Projection(error.to_string()))
        })
        .collect()
}

fn canonical_value(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_default()
}

fn auxiliary_rows(
    projections: &super::CanonicalProjections,
) -> Result<Vec<(String, usize, String)>, CheckpointError> {
    let groups = [
        ("leases", &projections.leases),
        ("workflows", &projections.workflows),
        ("metadata", &projections.metadata),
        ("conflicts", &projections.conflicts),
    ];
    let mut rows = Vec::new();
    for (kind, values) in groups {
        for (ordinal, value) in values.iter().enumerate() {
            let payload = serde_json::to_string(value)
                .map_err(|error| CheckpointError::Projection(error.to_string()))?;
            rows.push((kind.to_string(), ordinal, payload));
        }
    }
    Ok(rows)
}

fn validate_unique_records(
    active: &ActiveSnapshot,
    cold: &ColdSnapshot,
    edges: &[EdgeProjection],
) -> Result<(), CheckpointError> {
    let mut ids = HashSet::new();
    for id in active
        .hot
        .iter()
        .map(|row| &row.id)
        .chain(active.warm.iter().map(|row| &row.id))
        .chain(cold.cold.iter().map(|row| &row.id))
    {
        if !ids.insert(id) {
            return Err(CheckpointError::Projection(
                "knot appears in multiple cache tiers".to_string(),
            ));
        }
    }
    let unique_edges = edges
        .iter()
        .map(|edge| (&edge.src, &edge.kind, &edge.dst))
        .collect::<HashSet<_>>();
    if unique_edges.len() != edges.len() {
        return Err(CheckpointError::Projection("duplicate edge".to_string()));
    }
    Ok(())
}
