use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FsckIssue {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FsckReport {
    pub files_scanned: u64,
    pub issues: Vec<FsckIssue>,
}

impl FsckReport {
    pub fn ok(&self) -> bool {
        self.issues.is_empty()
    }
}

#[derive(Debug)]
pub enum FsckError {
    Io(std::io::Error),
}

impl fmt::Display for FsckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FsckError::Io(err) => write!(f, "I/O error: {}", err),
        }
    }
}

impl Error for FsckError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            FsckError::Io(err) => Some(err),
        }
    }
}

impl From<std::io::Error> for FsckError {
    fn from(value: std::io::Error) -> Self {
        FsckError::Io(value)
    }
}

pub fn run_fsck(repo_root: &Path) -> Result<FsckReport, FsckError> {
    let mut issues = Vec::new();
    let mut files = collect_json_files(repo_root)?;
    files.sort();

    let mut event_id_to_path: HashMap<String, PathBuf> = HashMap::new();
    let mut known_knot_ids: HashSet<String> = HashSet::new();
    let mut edge_refs = Vec::new();

    for path in &files {
        let raw = match std::fs::read(path) {
            Ok(value) => value,
            Err(err) => {
                issues.push(issue(path, &format!("unable to read file: {}", err)));
                continue;
            }
        };

        let value: Value = match serde_json::from_slice(&raw) {
            Ok(value) => value,
            Err(err) => {
                issues.push(issue(path, &format!("invalid JSON payload: {}", err)));
                continue;
            }
        };

        let Some(object) = value.as_object() else {
            issues.push(issue(path, "event payload must be a JSON object"));
            continue;
        };

        let event_id = match object.get("event_id").and_then(Value::as_str) {
            Some(value) if !value.trim().is_empty() => value.trim().to_string(),
            _ => {
                issues.push(issue(path, "missing required string field 'event_id'"));
                continue;
            }
        };

        if let Some(previous) = event_id_to_path.get(&event_id) {
            if previous != path {
                issues.push(issue(
                    path,
                    &format!(
                        "duplicate event_id '{}' also found in '{}'",
                        event_id,
                        previous.display()
                    ),
                ));
            }
        } else {
            event_id_to_path.insert(event_id.clone(), path.clone());
        }

        if object
            .get("occurred_at")
            .and_then(Value::as_str)
            .is_none_or(|value| value.trim().is_empty())
        {
            issues.push(issue(path, "missing required string field 'occurred_at'"));
        }

        if object
            .get("type")
            .and_then(Value::as_str)
            .is_none_or(|value| value.trim().is_empty())
        {
            issues.push(issue(path, "missing required string field 'type'"));
        }
        if let Some(event_type) = object.get("type").and_then(Value::as_str) {
            validate_filename(path, &event_id, event_type, &mut issues);
        }

        if !object.get("data").is_some_and(Value::is_object) {
            issues.push(issue(path, "missing required object field 'data'"));
            continue;
        }

        let event_type = object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let Some(data) = object.get("data").and_then(Value::as_object) else {
            continue;
        };

        if path_is_index(path) {
            if event_type == "idx.knot_head" {
                let knot_id = require_data_string(data, "knot_id", path, &mut issues);
                require_data_string(data, "title", path, &mut issues);
                require_data_string(data, "state", path, &mut issues);
                require_data_string(data, "updated_at", path, &mut issues);
                if let Some(knot_id) = knot_id {
                    known_knot_ids.insert(knot_id);
                }
            }
            continue;
        }

        let knot_id = match object.get("knot_id").and_then(Value::as_str) {
            Some(value) if !value.trim().is_empty() => value.trim().to_string(),
            _ => {
                issues.push(issue(path, "missing required string field 'knot_id'"));
                continue;
            }
        };
        known_knot_ids.insert(knot_id.clone());

        if matches!(event_type, "knot.edge_add" | "knot.edge_remove") {
            let dst = require_data_string(data, "dst", path, &mut issues);
            require_data_string(data, "kind", path, &mut issues);
            if let Some(dst) = dst {
                edge_refs.push((path.clone(), knot_id, dst));
            }
        }
    }

    for (path, src, dst) in edge_refs {
        if !known_knot_ids.contains(&src) {
            issues.push(issue(
                &path,
                &format!("edge source '{}' is not present in knot index", src),
            ));
        }
        if !known_knot_ids.contains(&dst) {
            issues.push(issue(
                &path,
                &format!("edge destination '{}' is not present in knot index", dst),
            ));
        }
    }

    Ok(FsckReport {
        files_scanned: files.len() as u64,
        issues,
    })
}

fn collect_json_files(repo_root: &Path) -> Result<Vec<PathBuf>, FsckError> {
    let mut files = Vec::new();
    for rel_root in [".knots/index", ".knots/events"] {
        let root = repo_root.join(rel_root);
        if !root.exists() {
            continue;
        }
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir)? {
                let path = entry?.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_some_and(|ext| ext == "json") {
                    files.push(path);
                }
            }
        }
    }
    Ok(files)
}

fn path_is_index(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == "index")
}

fn validate_filename(path: &Path, event_id: &str, event_type: &str, issues: &mut Vec<FsckIssue>) {
    let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
        issues.push(issue(path, "path has invalid filename"));
        return;
    };
    let expected = format!("{event_id}-{event_type}.json");
    if file_name != expected {
        issues.push(issue(
            path,
            &format!(
                "event filename mismatch: expected '{}', found '{}'",
                expected, file_name
            ),
        ));
    }
}

fn require_data_string(
    object: &serde_json::Map<String, Value>,
    key: &str,
    path: &Path,
    issues: &mut Vec<FsckIssue>,
) -> Option<String> {
    match object.get(key).and_then(Value::as_str) {
        Some(value) if !value.trim().is_empty() => Some(value.trim().to_string()),
        _ => {
            issues.push(issue(
                path,
                &format!("missing required string field data.{}", key),
            ));
            None
        }
    }
}

fn issue(path: &Path, message: &str) -> FsckIssue {
    FsckIssue {
        path: path.display().to_string(),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use uuid::Uuid;

    use super::run_fsck;

    fn unique_workspace() -> PathBuf {
        let root = std::env::temp_dir().join(format!("knots-fsck-test-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&root).expect("workspace should be creatable");
        root
    }

    #[test]
    fn reports_duplicate_event_ids() {
        let root = unique_workspace();
        let idx_path = root
            .join(".knots")
            .join("index")
            .join("2026")
            .join("02")
            .join("24")
            .join("dup-idx.knot_head.json");
        let full_path = root
            .join(".knots")
            .join("events")
            .join("2026")
            .join("02")
            .join("24")
            .join("dup-knot.description_set.json");
        std::fs::create_dir_all(
            idx_path
                .parent()
                .expect("index parent directory should exist"),
        )
        .expect("index parent should be creatable");
        std::fs::create_dir_all(
            full_path
                .parent()
                .expect("full parent directory should exist"),
        )
        .expect("full parent should be creatable");

        std::fs::write(
            &idx_path,
            concat!(
                "{\n",
                "  \"event_id\": \"dup\",\n",
                "  \"occurred_at\": \"2026-02-24T10:00:00Z\",\n",
                "  \"type\": \"idx.knot_head\",\n",
                "  \"data\": {\n",
                "    \"knot_id\": \"K-1\",\n",
                "    \"title\": \"Title\",\n",
                "    \"state\": \"work_item\",\n",
                "    \"updated_at\": \"2026-02-24T10:00:00Z\"\n",
                "  }\n",
                "}\n"
            ),
        )
        .expect("index event should write");

        std::fs::write(
            &full_path,
            concat!(
                "{\n",
                "  \"event_id\": \"dup\",\n",
                "  \"occurred_at\": \"2026-02-24T10:00:01Z\",\n",
                "  \"knot_id\": \"K-1\",\n",
                "  \"type\": \"knot.description_set\",\n",
                "  \"data\": {\"description\": \"x\"}\n",
                "}\n"
            ),
        )
        .expect("full event should write");

        let report = run_fsck(&root).expect("fsck should run");
        assert!(!report.ok());
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.message.contains("duplicate event_id")));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reports_missing_edge_destination_reference() {
        let root = unique_workspace();
        let idx_path = root
            .join(".knots")
            .join("index")
            .join("2026")
            .join("02")
            .join("24")
            .join("1000-idx.knot_head.json");
        let edge_path = root
            .join(".knots")
            .join("events")
            .join("2026")
            .join("02")
            .join("24")
            .join("1001-knot.edge_add.json");
        std::fs::create_dir_all(
            idx_path
                .parent()
                .expect("index parent directory should exist"),
        )
        .expect("index parent should be creatable");
        std::fs::create_dir_all(
            edge_path
                .parent()
                .expect("edge parent directory should exist"),
        )
        .expect("edge parent should be creatable");

        std::fs::write(
            &idx_path,
            concat!(
                "{\n",
                "  \"event_id\": \"1000\",\n",
                "  \"occurred_at\": \"2026-02-24T10:00:00Z\",\n",
                "  \"type\": \"idx.knot_head\",\n",
                "  \"data\": {\n",
                "    \"knot_id\": \"K-src\",\n",
                "    \"title\": \"Source\",\n",
                "    \"state\": \"work_item\",\n",
                "    \"updated_at\": \"2026-02-24T10:00:00Z\"\n",
                "  }\n",
                "}\n"
            ),
        )
        .expect("index event should write");

        std::fs::write(
            &edge_path,
            concat!(
                "{\n",
                "  \"event_id\": \"1001\",\n",
                "  \"occurred_at\": \"2026-02-24T10:00:01Z\",\n",
                "  \"knot_id\": \"K-src\",\n",
                "  \"type\": \"knot.edge_add\",\n",
                "  \"data\": {\n",
                "    \"kind\": \"blocked_by\",\n",
                "    \"dst\": \"K-missing\"\n",
                "  }\n",
                "}\n"
            ),
        )
        .expect("edge event should write");

        let report = run_fsck(&root).expect("fsck should run");
        assert!(!report.ok());
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.message.contains("destination")));

        let _ = std::fs::remove_dir_all(root);
    }
}
