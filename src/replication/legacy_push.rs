use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::db;
use crate::project::StorePaths;
use crate::sync::SyncError;

const BASELINE_KEY: &str = "legacy_push_baseline_v1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct Baseline {
    physical_count: u64,
    physical_fingerprint: String,
    outbox_rowid: i64,
}

pub(super) struct Selection {
    pub(super) files: Vec<PathBuf>,
    pub(super) physical_count: u64,
    pub(super) baseline: Option<Baseline>,
}

#[derive(Default)]
struct Inventory {
    count: u64,
    fingerprint: u128,
    files: Vec<PathBuf>,
}

pub(super) fn select(conn: &Connection, paths: &StorePaths) -> Result<Selection, SyncError> {
    let baseline = load_baseline(conn)?;
    let Some(baseline) = baseline else {
        return reconcile(conn, paths, 0);
    };
    let physical = scan(paths, false)?;
    let receipts = receipt_files(conn, paths, baseline.outbox_rowid)?;
    let expected_count = baseline
        .physical_count
        .checked_add(receipts.existing.len() as u64)
        .ok_or_else(invalid_baseline)?;
    let expected_fingerprint =
        parse_fingerprint(&baseline.physical_fingerprint)? ^ receipts.fingerprint;
    if physical.count != expected_count || physical.fingerprint != expected_fingerprint {
        return reconcile(conn, paths, baseline.outbox_rowid);
    }
    let next = receipts.complete.then(|| Baseline {
        physical_count: physical.count,
        physical_fingerprint: format_fingerprint(physical.fingerprint),
        outbox_rowid: receipts.max_rowid,
    });
    Ok(Selection {
        files: receipts.existing,
        physical_count: physical.count,
        baseline: next,
    })
}

pub(super) fn save_baseline(conn: &Connection, baseline: &Baseline) -> Result<(), SyncError> {
    let value = serde_json::to_string(baseline).map_err(|error| SyncError::InvalidEvent {
        path: PathBuf::from(BASELINE_KEY),
        message: error.to_string(),
    })?;
    db::set_meta(conn, BASELINE_KEY, &value)?;
    Ok(())
}

fn reconcile(
    conn: &Connection,
    paths: &StorePaths,
    prior_rowid: i64,
) -> Result<Selection, SyncError> {
    let physical = scan(paths, true)?;
    let receipts = receipt_files(conn, paths, prior_rowid)?;
    let next = receipts.complete.then(|| Baseline {
        physical_count: physical.count,
        physical_fingerprint: format_fingerprint(physical.fingerprint),
        outbox_rowid: receipts.max_rowid,
    });
    Ok(Selection {
        files: physical.files,
        physical_count: physical.count,
        baseline: next,
    })
}

fn load_baseline(conn: &Connection) -> Result<Option<Baseline>, SyncError> {
    db::get_meta(conn, BASELINE_KEY)?
        .map(|value| {
            serde_json::from_str(&value).map_err(|error| SyncError::InvalidEvent {
                path: PathBuf::from(BASELINE_KEY),
                message: format!("invalid legacy push baseline: {error}"),
            })
        })
        .transpose()
}

struct ReceiptFiles {
    existing: Vec<PathBuf>,
    fingerprint: u128,
    max_rowid: i64,
    complete: bool,
}

fn receipt_files(
    conn: &Connection,
    paths: &StorePaths,
    after_rowid: i64,
) -> Result<ReceiptFiles, SyncError> {
    let rows = db::list_outbox_paths_after(conn, after_rowid)?;
    let mut existing = Vec::new();
    let mut fingerprint = 0u128;
    let mut complete = true;
    let mut max_rowid = after_rowid;
    for (rowid, relative) in rows {
        max_rowid = rowid;
        let relative = validate_outbox_path(&relative)?;
        let absolute = paths.root.join(&relative);
        if !absolute.exists() {
            complete = false;
            continue;
        }
        fingerprint ^= file_fingerprint(&absolute, &relative)?;
        existing.push(Path::new(".knots").join(relative));
    }
    existing.sort();
    Ok(ReceiptFiles {
        existing,
        fingerprint,
        max_rowid,
        complete,
    })
}

fn scan(paths: &StorePaths, collect: bool) -> Result<Inventory, SyncError> {
    let mut inventory = Inventory::default();
    for relative_root in ["index", "events", "snapshots"] {
        let root = paths.root.join(relative_root);
        if !root.exists() {
            continue;
        }
        let mut directories = vec![root];
        while let Some(directory) = directories.pop() {
            for entry in std::fs::read_dir(directory)? {
                let path = entry?.path();
                if path.is_dir() {
                    directories.push(path);
                    continue;
                }
                if path.extension().is_none_or(|extension| extension != "json") {
                    continue;
                }
                let relative =
                    path.strip_prefix(&paths.root)
                        .map_err(|error| SyncError::InvalidEvent {
                            path: path.clone(),
                            message: format!("failed to relativize event file: {error}"),
                        })?;
                inventory.count += 1;
                inventory.fingerprint ^= file_fingerprint(&path, relative)?;
                if collect {
                    inventory.files.push(Path::new(".knots").join(relative));
                }
            }
        }
    }
    inventory.files.sort();
    Ok(inventory)
}

fn file_fingerprint(path: &Path, relative: &Path) -> Result<u128, SyncError> {
    let metadata = std::fs::metadata(path)?;
    let modified = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let mut digest = Sha256::new();
    digest.update(relative.as_os_str().as_encoded_bytes());
    digest.update(metadata.len().to_le_bytes());
    digest.update(modified.as_nanos().to_le_bytes());
    let bytes = digest.finalize();
    Ok(u128::from_le_bytes(
        bytes[..16].try_into().expect("fixed digest"),
    ))
}

fn validate_outbox_path(value: &str) -> Result<PathBuf, SyncError> {
    let path = Path::new(value);
    let valid_root = matches!(path.components().next(), Some(Component::Normal(root))
        if root == "events" || root == "index");
    let normal = path
        .components()
        .all(|component| matches!(component, Component::Normal(_)));
    if path
        .extension()
        .is_some_and(|extension| extension == "json")
        && valid_root
        && normal
    {
        return Ok(path.to_path_buf());
    }
    Err(SyncError::InvalidEvent {
        path: path.to_path_buf(),
        message: "invalid durable outbox path".to_string(),
    })
}

fn parse_fingerprint(value: &str) -> Result<u128, SyncError> {
    u128::from_str_radix(value, 16).map_err(|_| invalid_baseline())
}

fn format_fingerprint(value: u128) -> String {
    format!("{value:032x}")
}

fn invalid_baseline() -> SyncError {
    SyncError::InvalidEvent {
        path: PathBuf::from(BASELINE_KEY),
        message: "invalid legacy push baseline".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::{save_baseline, select};
    use crate::db;
    use crate::project::StorePaths;

    #[test]
    fn established_baseline_selects_only_new_durable_receipts() {
        let fixture = Fixture::new("legacy-push-incremental");
        fixture.write("events/2026/08/28/legacy.json", b"legacy");
        fixture.establish();
        fixture.receipt("new", "events/2026/08/28/new.json", b"new");

        let selected = select(&fixture.conn, &fixture.paths).expect("select incrementally");

        assert_eq!(selected.physical_count, 2);
        assert_eq!(selected.files, [path("events/2026/08/28/new.json")]);
        assert!(selected.baseline.is_some());
    }

    #[test]
    fn unreceipted_legacy_write_forces_complete_reconciliation() {
        let fixture = Fixture::new("legacy-push-old-writer");
        fixture.write("events/2026/08/28/first.json", b"first");
        fixture.establish();
        fixture.write("index/2026/08/28/old-writer.json", b"old writer");

        let selected = select(&fixture.conn, &fixture.paths).expect("reconcile legacy write");

        assert_eq!(selected.physical_count, 2);
        assert_eq!(selected.files.len(), 2);
        assert!(selected
            .files
            .contains(&path("events/2026/08/28/first.json")));
        assert!(selected
            .files
            .contains(&path("index/2026/08/28/old-writer.json")));
    }

    #[test]
    fn missing_receipt_file_never_advances_the_durable_baseline() {
        let fixture = Fixture::new("legacy-push-receipt-gap");
        fixture.establish();
        db::record_outbox_event(
            &fixture.conn,
            "missing",
            "events",
            "events/2026/08/28/missing.json",
            &digest(b"missing"),
            b"missing",
        )
        .expect("record interrupted write");

        let selected = select(&fixture.conn, &fixture.paths).expect("select receipt gap");

        assert!(selected.files.is_empty());
        assert!(selected.baseline.is_none());
    }

    struct Fixture {
        _workspace: knots_test_support::TestWorkspace,
        conn: rusqlite::Connection,
        paths: StorePaths,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let workspace = knots_test_support::workspace(name);
            let paths = StorePaths {
                root: workspace.path().join(".knots"),
            };
            Self {
                _workspace: workspace,
                conn: db::open_connection(":memory:").expect("open database"),
                paths,
            }
        }

        fn establish(&self) {
            let selected = select(&self.conn, &self.paths).expect("initial reconciliation");
            save_baseline(
                &self.conn,
                selected.baseline.as_ref().expect("complete baseline"),
            )
            .expect("save baseline");
        }

        fn receipt(&self, event_id: &str, relative: &str, payload: &[u8]) {
            db::record_outbox_event(
                &self.conn,
                event_id,
                "events",
                relative,
                &digest(payload),
                payload,
            )
            .expect("record receipt");
            self.write(relative, payload);
        }

        fn write(&self, relative: &str, payload: &[u8]) {
            let target = self.paths.root.join(relative);
            std::fs::create_dir_all(target.parent().expect("event parent"))
                .expect("create event parent");
            std::fs::write(target, payload).expect("write event");
        }
    }

    fn path(relative: &str) -> std::path::PathBuf {
        std::path::Path::new(".knots").join(relative)
    }

    fn digest(payload: &[u8]) -> String {
        format!("{:x}", Sha256::digest(payload))
    }
}
