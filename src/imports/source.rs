use std::path::Path;
use std::str::FromStr;

use serde::Deserialize;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::domain::state::KnotState;

use super::errors::ImportError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Jsonl,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::Jsonl => "jsonl",
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
    pub profile_id: Option<String>,
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
        Some(value) if value == "in_progress" || value == "in-progress" => {
            KnotState::Implementation
        }
        Some(value) if value == "blocked" => KnotState::ReadyForImplementation,
        Some(value) if value == "open" => KnotState::ReadyForImplementation,
        _ => KnotState::ReadyForImplementation,
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

#[cfg(test)]
mod tests {
    use super::{
        map_dependency_kind, map_source_state, merged_body, normalize_path, parse_since,
        parse_timestamp, source_key, SourceDependency, SourceIssue, SourceKind,
        SourceMetadataEntry, SourceNotesField,
    };
    use crate::domain::state::KnotState;
    use crate::imports::errors::ImportError;
    use serde_json::json;

    fn base_issue() -> SourceIssue {
        SourceIssue {
            id: "ISSUE-1".to_string(),
            title: "Title".to_string(),
            profile_id: None,
            workflow_id: Some("default".to_string()),
            description: None,
            body: None,
            notes: None,
            handoff_capsules: Vec::new(),
            state: None,
            status: None,
            priority: None,
            owner: None,
            created_by: None,
            issue_type: None,
            type_name: None,
            labels: Vec::new(),
            tags: Vec::new(),
            dependencies: Vec::new(),
            created_at: None,
            updated_at: None,
            closed_at: None,
            close_reason: None,
        }
    }

    #[test]
    fn source_kind_and_key_are_stable() {
        assert_eq!(SourceKind::Jsonl.as_str(), "jsonl");
        assert_eq!(
            source_key(SourceKind::Jsonl, "/tmp/issues.jsonl"),
            "jsonl:/tmp/issues.jsonl"
        );
    }

    #[test]
    fn normalize_path_handles_absolute_and_relative_paths() {
        let root = std::env::temp_dir().join(format!("knots-source-path-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&root).expect("root should be creatable");
        let existing = root.join("issues.jsonl");
        std::fs::write(&existing, "{}\n").expect("fixture should be writable");
        let canonical_existing = std::fs::canonicalize(&existing)
            .expect("fixture canonical path should resolve")
            .to_string_lossy()
            .to_string();

        let absolute = normalize_path(existing.to_str().expect("utf8 path"))
            .expect("absolute path should normalize");
        assert_eq!(absolute, canonical_existing);

        let previous = std::env::current_dir().expect("cwd should be readable");
        std::env::set_current_dir(&root).expect("cwd should update");
        let relative = normalize_path("issues.jsonl").expect("relative path should normalize");
        assert_eq!(relative, canonical_existing);
        let missing =
            normalize_path("missing.jsonl").expect("missing relative path should normalize");
        let expected_missing = std::env::current_dir()
            .expect("cwd should be readable")
            .join("missing.jsonl")
            .to_string_lossy()
            .to_string();
        assert_eq!(missing, expected_missing);
        std::env::set_current_dir(previous).expect("cwd should restore");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn parse_timestamp_and_since_validate_rfc3339() {
        assert_eq!(
            parse_timestamp(Some("2026-02-25T10:00:00Z")).as_deref(),
            Some("2026-02-25T10:00:00Z")
        );
        assert_eq!(parse_timestamp(Some("not-rfc3339")), None);
        assert!(parse_since(None).expect("none should parse").is_none());
        assert!(parse_since(Some("2026-02-25T10:00:00Z"))
            .expect("valid since should parse")
            .is_some());
        assert!(matches!(
            parse_since(Some("invalid")),
            Err(ImportError::InvalidTimestamp(_))
        ));
    }

    #[test]
    fn state_mapping_prefers_explicit_state_and_maps_statuses() {
        let mut explicit = base_issue();
        explicit.state = Some("implementing".to_string());
        explicit.status = Some("closed".to_string());
        assert_eq!(
            map_source_state(&explicit).expect("explicit state should parse"),
            KnotState::Implementation
        );

        let mut closed = base_issue();
        closed.status = Some("closed".to_string());
        assert_eq!(
            map_source_state(&closed).expect("closed should map"),
            KnotState::Shipped
        );

        let mut in_progress = base_issue();
        in_progress.status = Some("in_progress".to_string());
        assert_eq!(
            map_source_state(&in_progress).expect("in_progress should map"),
            KnotState::Implementation
        );
    }

    #[test]
    fn dependency_kind_and_body_merge_handle_known_shapes() {
        assert_eq!(map_dependency_kind(Some("parent-child")), "parent_of");
        assert_eq!(map_dependency_kind(Some("blocks")), "blocked_by");
        assert_eq!(map_dependency_kind(Some("related")), "related");
        assert_eq!(map_dependency_kind(Some("unknown")), "blocked_by");

        let mut issue = base_issue();
        issue.description = Some("A".to_string());
        issue.body = Some("B".to_string());
        assert_eq!(merged_body(&issue).as_deref(), Some("A\n\nB"));
        issue.description = Some("   ".to_string());
        issue.body = None;
        assert!(merged_body(&issue).is_none());
    }

    #[test]
    fn source_issue_defaults_deserialize_for_optional_fields() {
        let row = json!({
            "id": "D-2",
            "title": "Defaults"
        });
        let parsed: SourceIssue =
            serde_json::from_value(row).expect("minimal row should deserialize");
        assert!(parsed.labels.is_empty());
        assert!(parsed.tags.is_empty());
        assert!(parsed.handoff_capsules.is_empty());
        assert!(parsed.dependencies.is_empty());
        assert!(parsed.notes.is_none());
    }

    #[test]
    fn source_types_serialize_expected_shapes() {
        let dep = SourceDependency {
            depends_on_id: Some("A".to_string()),
            dep_type: Some("blocks".to_string()),
        };
        assert_eq!(dep.depends_on_id.as_deref(), Some("A"));
        let note = SourceNotesField::Text("legacy".to_string());
        assert!(matches!(note, SourceNotesField::Text(_)));
        let entry = SourceMetadataEntry {
            entry_id: Some("e1".to_string()),
            content: Some("note".to_string()),
            username: Some("u".to_string()),
            datetime: Some("2026-02-25T10:00:00Z".to_string()),
            agentname: Some("a".to_string()),
            model: Some("m".to_string()),
            version: Some("v".to_string()),
        };
        assert_eq!(entry.entry_id.as_deref(), Some("e1"));
    }
}
