use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::db::{self, ColdCatalogRecord, KnotCacheRecord, UpsertKnotHot, WarmKnotRecord};

const SNAPSHOT_SCHEMA_VERSION: i64 = 1;
const ACTIVE_SUFFIX: &str = "-active_catalog.snapshot.json";
const COLD_SUFFIX: &str = "-cold_catalog.snapshot.json";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SnapshotWriteSummary {
    pub active_path: PathBuf,
    pub cold_path: PathBuf,
    pub hot_count: u64,
    pub warm_count: u64,
    pub cold_count: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SnapshotLoadSummary {
    pub active_path: Option<PathBuf>,
    pub cold_path: Option<PathBuf>,
    pub hot_count: u64,
    pub warm_count: u64,
    pub cold_count: u64,
}

#[derive(Debug)]
pub enum SnapshotError {
    Io(std::io::Error),
    Db(rusqlite::Error),
    Json(serde_json::Error),
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapshotError::Io(err) => write!(f, "I/O error: {}", err),
            SnapshotError::Db(err) => write!(f, "database error: {}", err),
            SnapshotError::Json(err) => write!(f, "JSON error: {}", err),
        }
    }
}

impl Error for SnapshotError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            SnapshotError::Io(err) => Some(err),
            SnapshotError::Db(err) => Some(err),
            SnapshotError::Json(err) => Some(err),
        }
    }
}

impl From<std::io::Error> for SnapshotError {
    fn from(value: std::io::Error) -> Self {
        SnapshotError::Io(value)
    }
}

impl From<rusqlite::Error> for SnapshotError {
    fn from(value: rusqlite::Error) -> Self {
        SnapshotError::Db(value)
    }
}

impl From<serde_json::Error> for SnapshotError {
    fn from(value: serde_json::Error) -> Self {
        SnapshotError::Json(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ActiveCatalogSnapshot {
    schema_version: i64,
    written_at: String,
    hot: Vec<KnotCacheRecord>,
    warm: Vec<WarmKnotRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ColdCatalogSnapshot {
    schema_version: i64,
    written_at: String,
    cold: Vec<ColdCatalogRecord>,
}

pub fn write_snapshots(
    conn: &Connection,
    repo_root: &Path,
) -> Result<SnapshotWriteSummary, SnapshotError> {
    let hot = db::list_knot_hot(conn)?;
    let warm = db::list_knot_warm(conn)?;
    let cold = db::list_cold_catalog(conn)?;

    let written_at = current_rfc3339();
    let stamp = filename_timestamp();
    let snapshots_dir = repo_root.join(".knots").join("snapshots");
    std::fs::create_dir_all(&snapshots_dir)?;

    let active = ActiveCatalogSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        written_at: written_at.clone(),
        hot: hot.clone(),
        warm: warm.clone(),
    };
    let cold_snapshot = ColdCatalogSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        written_at,
        cold: cold.clone(),
    };

    let active_path = snapshots_dir.join(format!("{stamp}{ACTIVE_SUFFIX}"));
    let cold_path = snapshots_dir.join(format!("{stamp}{COLD_SUFFIX}"));

    std::fs::write(&active_path, serde_json::to_vec_pretty(&active)?)?;
    std::fs::write(&cold_path, serde_json::to_vec_pretty(&cold_snapshot)?)?;

    Ok(SnapshotWriteSummary {
        active_path,
        cold_path,
        hot_count: hot.len() as u64,
        warm_count: warm.len() as u64,
        cold_count: cold.len() as u64,
    })
}

pub fn apply_latest_snapshots(
    conn: &Connection,
    repo_root: &Path,
) -> Result<SnapshotLoadSummary, SnapshotError> {
    let snapshots_dir = repo_root.join(".knots").join("snapshots");
    if !snapshots_dir.exists() {
        return Ok(SnapshotLoadSummary {
            active_path: None,
            cold_path: None,
            hot_count: 0,
            warm_count: 0,
            cold_count: 0,
        });
    }

    let active_path = latest_snapshot_path(&snapshots_dir, ACTIVE_SUFFIX)?;
    let cold_path = latest_snapshot_path(&snapshots_dir, COLD_SUFFIX)?;
    let mut hot_count = 0u64;
    let mut warm_count = 0u64;
    let mut cold_count = 0u64;

    if let Some(path) = active_path.as_ref() {
        let payload = std::fs::read(path)?;
        let snapshot: ActiveCatalogSnapshot = serde_json::from_slice(&payload)?;
        for record in &snapshot.hot {
            db::upsert_knot_hot(
                conn,
                &UpsertKnotHot {
                    id: &record.id,
                    title: &record.title,
                    state: &record.state,
                    updated_at: &record.updated_at,
                    body: record.body.as_deref(),
                    description: record.description.as_deref(),
                    priority: record.priority,
                    knot_type: record.knot_type.as_deref(),
                    tags: &record.tags,
                    notes: &record.notes,
                    handoff_capsules: &record.handoff_capsules,
                    workflow_id: &record.workflow_id,
                    workflow_etag: record.workflow_etag.as_deref(),
                    created_at: record.created_at.as_deref(),
                },
            )?;
            hot_count += 1;
        }

        for record in &snapshot.warm {
            db::upsert_knot_warm(conn, &record.id, &record.title)?;
            warm_count += 1;
        }
    }

    if let Some(path) = cold_path.as_ref() {
        let payload = std::fs::read(path)?;
        let snapshot: ColdCatalogSnapshot = serde_json::from_slice(&payload)?;
        for record in &snapshot.cold {
            db::upsert_cold_catalog(
                conn,
                &record.id,
                &record.title,
                &record.state,
                &record.updated_at,
            )?;
            cold_count += 1;
        }
    }

    Ok(SnapshotLoadSummary {
        active_path,
        cold_path,
        hot_count,
        warm_count,
        cold_count,
    })
}

fn latest_snapshot_path(dir: &Path, suffix: &str) -> Result<Option<PathBuf>, SnapshotError> {
    let mut matches = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if name.ends_with(suffix) {
            matches.push(path);
        }
    }
    matches.sort();
    Ok(matches.pop())
}

fn current_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("RFC3339 formatting should not fail")
}

fn filename_timestamp() -> String {
    let now = OffsetDateTime::now_utc();
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::path::PathBuf;

    use uuid::Uuid;

    use super::{apply_latest_snapshots, latest_snapshot_path, write_snapshots, SnapshotError};
    use crate::db::{self, UpsertKnotHot};

    fn unique_workspace() -> PathBuf {
        let root = std::env::temp_dir().join(format!("knots-snapshot-test-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&root).expect("workspace should be creatable");
        root
    }

    #[test]
    fn writes_and_loads_snapshots() {
        let root = unique_workspace();
        let db_path = root.join(".knots/cache/state.sqlite");
        std::fs::create_dir_all(
            db_path
                .parent()
                .expect("db parent should exist for snapshot test"),
        )
        .expect("db parent should be creatable");

        let conn = db::open_connection(db_path.to_str().expect("utf8 path"))
            .expect("snapshot db should open");
        db::upsert_knot_hot(
            &conn,
            &UpsertKnotHot {
                id: "K-hot",
                title: "Hot",
                state: "work_item",
                updated_at: "2026-02-24T10:00:00Z",
                body: Some("hot body"),
                description: Some("hot body"),
                priority: Some(1),
                knot_type: Some("task"),
                tags: &["ops".to_string()],
                notes: &[],
                handoff_capsules: &[],
                workflow_id: "default",
                workflow_etag: Some("evt-1"),
                created_at: Some("2026-02-24T10:00:00Z"),
            },
        )
        .expect("hot upsert should succeed");
        db::upsert_knot_warm(&conn, "K-warm", "Warm").expect("warm upsert should succeed");
        db::upsert_cold_catalog(&conn, "K-cold", "Cold", "shipped", "2026-02-24T10:01:00Z")
            .expect("cold upsert should succeed");

        let written = write_snapshots(&conn, &root).expect("snapshot write should succeed");
        assert!(written.active_path.exists());
        assert!(written.cold_path.exists());

        let root2 = unique_workspace();
        let db2_path = root2.join(".knots/cache/state.sqlite");
        std::fs::create_dir_all(
            db2_path
                .parent()
                .expect("db parent should exist for restore test"),
        )
        .expect("restore db parent should be creatable");
        let conn2 = db::open_connection(db2_path.to_str().expect("utf8 path"))
            .expect("restore db should open");

        let snapshots_target = root2.join(".knots/snapshots");
        std::fs::create_dir_all(&snapshots_target).expect("snapshot target should exist");
        std::fs::copy(
            &written.active_path,
            snapshots_target.join(
                written
                    .active_path
                    .file_name()
                    .expect("active filename should exist"),
            ),
        )
        .expect("active snapshot should copy");
        std::fs::copy(
            &written.cold_path,
            snapshots_target.join(
                written
                    .cold_path
                    .file_name()
                    .expect("cold filename should exist"),
            ),
        )
        .expect("cold snapshot should copy");

        let loaded = apply_latest_snapshots(&conn2, &root2).expect("snapshot load should succeed");
        assert_eq!(loaded.hot_count, 1);
        assert_eq!(loaded.warm_count, 1);
        assert_eq!(loaded.cold_count, 1);

        let hot = db::get_knot_hot(&conn2, "K-hot")
            .expect("hot query should succeed")
            .expect("hot knot should exist");
        assert_eq!(hot.title, "Hot");

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(root2);
    }

    #[test]
    fn snapshot_error_display_source_and_from_cover_variants() {
        let io: SnapshotError = std::io::Error::other("disk").into();
        assert!(io.to_string().contains("I/O error"));
        assert!(io.source().is_some());

        let db: SnapshotError = rusqlite::Error::InvalidQuery.into();
        assert!(db.to_string().contains("database error"));
        assert!(db.source().is_some());

        let json_err = serde_json::from_slice::<serde_json::Value>(b"{")
            .expect_err("invalid json should fail");
        let json: SnapshotError = json_err.into();
        assert!(json.to_string().contains("JSON error"));
        assert!(json.source().is_some());
    }

    #[test]
    fn latest_snapshot_path_skips_directories_and_invalid_filenames() {
        let root = unique_workspace();
        let snapshots = root.join(".knots/snapshots");
        std::fs::create_dir_all(&snapshots).expect("snapshots directory should be creatable");

        std::fs::create_dir_all(snapshots.join("20260225T100000Z-active_catalog.snapshot.json"))
            .expect("directory fixture should be creatable");
        std::fs::write(
            snapshots.join("20260225T100001Z-active_catalog.snapshot.json"),
            b"{}",
        )
        .expect("older snapshot should write");
        std::fs::write(
            snapshots.join("20260225T100002Z-active_catalog.snapshot.json"),
            b"{}",
        )
        .expect("latest snapshot should write");

        #[cfg(unix)]
        {
            use std::ffi::OsString;
            use std::os::unix::ffi::OsStringExt;
            let mut bytes = b"invalid-utf8-".to_vec();
            bytes.push(0xFF);
            bytes.extend_from_slice(b"-active_catalog.snapshot.json");
            let non_utf8 = OsString::from_vec(bytes);
            let _ = std::fs::write(snapshots.join(non_utf8), b"{}");
        }

        let latest = latest_snapshot_path(&snapshots, "-active_catalog.snapshot.json")
            .expect("latest snapshot lookup should succeed")
            .expect("latest snapshot should exist");
        assert!(latest
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.starts_with("20260225T100002Z")));

        let _ = std::fs::remove_dir_all(root);
    }
}
