use std::path::Path;
use std::process::Command;
use std::str::FromStr;

use serde::Deserialize;
use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::domain::state::KnotState;

use super::errors::ImportError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Jsonl,
    Dolt,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::Jsonl => "jsonl",
            SourceKind::Dolt => "dolt",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SourceDependency {
    #[serde(default)]
    pub depends_on_id: Option<String>,
    #[serde(default, rename = "type")]
    pub dep_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SourceIssue {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub workflow_id: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub notes: Option<SourceNotesField>,
    #[serde(default)]
    pub handoff_capsules: Vec<SourceMetadataEntry>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub priority: Option<i64>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub created_by: Option<String>,
    #[serde(default, rename = "issue_type")]
    pub issue_type: Option<String>,
    #[serde(default, rename = "type")]
    pub type_name: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<SourceDependency>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub closed_at: Option<String>,
    #[serde(default)]
    pub close_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum SourceNotesField {
    Text(String),
    Entries(Vec<SourceMetadataEntry>),
}

#[derive(Debug, Clone, Deserialize)]
pub struct SourceMetadataEntry {
    #[serde(default)]
    pub entry_id: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub datetime: Option<String>,
    #[serde(default)]
    pub agentname: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

pub fn normalize_path(raw: &str) -> Result<String, ImportError> {
    let path = Path::new(raw);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let normalized = if absolute.exists() {
        absolute.canonicalize().unwrap_or(absolute)
    } else {
        absolute
    };
    Ok(normalized.to_string_lossy().to_string())
}

pub fn source_key(kind: SourceKind, source_ref: &str) -> String {
    format!("{}:{}", kind.as_str(), source_ref)
}

pub fn parse_timestamp(raw: Option<&str>) -> Option<String> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    OffsetDateTime::parse(raw, &Rfc3339)
        .ok()
        .and_then(|ts| ts.format(&Rfc3339).ok())
}

pub fn parse_since(since: Option<&str>) -> Result<Option<OffsetDateTime>, ImportError> {
    match since {
        Some(value) => Ok(Some(
            OffsetDateTime::parse(value, &Rfc3339)
                .map_err(|_| ImportError::InvalidTimestamp(value.to_string()))?,
        )),
        None => Ok(None),
    }
}

pub fn map_source_state(issue: &SourceIssue) -> Result<KnotState, ImportError> {
    if let Some(state) = issue.state.as_deref() {
        return Ok(KnotState::from_str(state)?);
    }

    let mapped = match issue
        .status
        .as_deref()
        .map(|s| s.trim().to_ascii_lowercase())
    {
        Some(value) if value == "closed" => KnotState::Shipped,
        Some(value) if value == "deferred" => KnotState::Deferred,
        Some(value) if value == "in_progress" || value == "in-progress" => KnotState::Implementing,
        Some(value) if value == "blocked" => KnotState::Refining,
        Some(value) if value == "open" => KnotState::WorkItem,
        _ => KnotState::WorkItem,
    };
    Ok(mapped)
}

pub fn map_dependency_kind(dep_type: Option<&str>) -> &'static str {
    match dep_type.map(|value| value.trim().to_ascii_lowercase()) {
        Some(value) if value == "parent-child" => "parent_of",
        Some(value) if value == "blocks" => "blocked_by",
        Some(value) if value == "related" => "related",
        _ => "blocked_by",
    }
}

pub fn merged_body(issue: &SourceIssue) -> Option<String> {
    let mut parts = Vec::new();
    for item in [&issue.description, &issue.body] {
        if let Some(value) = item.as_deref() {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                parts.push(trimmed.to_string());
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

pub fn ensure_dolt_available() -> Result<(), ImportError> {
    match Command::new("dolt").arg("--version").output() {
        Ok(output) if output.status.success() => Ok(()),
        _ => Err(ImportError::MissingDolt),
    }
}

pub fn fetch_dolt_rows(repo_root: &str) -> Result<Vec<Value>, ImportError> {
    let output = Command::new("dolt")
        .current_dir(repo_root)
        .args(["sql", "-r", "json", "-q", "SELECT * FROM issues"])
        .output()
        .map_err(ImportError::Io)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(ImportError::CommandFailed(format!(
            "dolt sql failed: {}",
            stderr
        )));
    }

    let value: Value = serde_json::from_slice(&output.stdout)?;
    if let Some(rows) = value.as_array() {
        return Ok(rows.clone());
    }
    if let Some(rows) = value.get("rows").and_then(|rows| rows.as_array()) {
        return Ok(rows.clone());
    }
    Err(ImportError::InvalidRecord(
        "unexpected dolt JSON output shape".to_string(),
    ))
}

pub fn source_issue_from_dolt_row(value: Value) -> Result<SourceIssue, ImportError> {
    match value {
        Value::Object(_) => serde_json::from_value(value)
            .map_err(|err| ImportError::InvalidRecord(format!("invalid dolt row: {}", err))),
        _ => Err(ImportError::InvalidRecord(
            "dolt row must be a JSON object".to_string(),
        )),
    }
}
