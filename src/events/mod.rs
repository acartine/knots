#![allow(dead_code)]

use std::error::Error;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventStream {
    Full,
    Index,
}

impl EventStream {
    fn root_dir(self) -> &'static str {
        match self {
            EventStream::Full => ".knots/events",
            EventStream::Index => ".knots/index",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullEventKind {
    KnotCreated,
    KnotTitleSet,
    KnotBodySet,
    KnotDescriptionSet,
    KnotStateSet,
    KnotPrioritySet,
    KnotTypeSet,
    KnotCommentAdded,
    KnotNoteAdded,
    KnotHandoffCapsuleAdded,
    KnotTagAdd,
    KnotTagRemove,
    KnotEdgeAdd,
    KnotEdgeRemove,
    KnotReviewDecision,
}

impl FullEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FullEventKind::KnotCreated => "knot.created",
            FullEventKind::KnotTitleSet => "knot.title_set",
            FullEventKind::KnotBodySet => "knot.body_set",
            FullEventKind::KnotDescriptionSet => "knot.description_set",
            FullEventKind::KnotStateSet => "knot.state_set",
            FullEventKind::KnotPrioritySet => "knot.priority_set",
            FullEventKind::KnotTypeSet => "knot.type_set",
            FullEventKind::KnotCommentAdded => "knot.comment_added",
            FullEventKind::KnotNoteAdded => "knot.note_added",
            FullEventKind::KnotHandoffCapsuleAdded => "knot.handoff_capsule_added",
            FullEventKind::KnotTagAdd => "knot.tag_add",
            FullEventKind::KnotTagRemove => "knot.tag_remove",
            FullEventKind::KnotEdgeAdd => "knot.edge_add",
            FullEventKind::KnotEdgeRemove => "knot.edge_remove",
            FullEventKind::KnotReviewDecision => "knot.review_decision",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexEventKind {
    KnotHead,
}

impl IndexEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            IndexEventKind::KnotHead => "idx.knot_head",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowPrecondition {
    pub workflow_etag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullEvent {
    pub event_id: String,
    pub occurred_at: String,
    pub knot_id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub data: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub precondition: Option<WorkflowPrecondition>,
}

impl FullEvent {
    pub fn new(knot_id: impl Into<String>, kind: FullEventKind, data: Value) -> Self {
        Self::with_identity(
            new_event_id(),
            now_utc_rfc3339(),
            knot_id,
            kind.as_str().to_string(),
            data,
        )
    }

    pub fn with_identity(
        event_id: impl Into<String>,
        occurred_at: impl Into<String>,
        knot_id: impl Into<String>,
        event_type: impl Into<String>,
        data: Value,
    ) -> Self {
        Self {
            event_id: event_id.into(),
            occurred_at: occurred_at.into(),
            knot_id: knot_id.into(),
            event_type: event_type.into(),
            data,
            precondition: None,
        }
    }

    pub fn with_precondition(mut self, workflow_etag: impl Into<String>) -> Self {
        self.precondition = Some(WorkflowPrecondition {
            workflow_etag: workflow_etag.into(),
        });
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEvent {
    pub event_id: String,
    pub occurred_at: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub data: Value,
}

impl IndexEvent {
    pub fn new(kind: IndexEventKind, data: Value) -> Self {
        Self::with_identity(
            new_event_id(),
            now_utc_rfc3339(),
            kind.as_str().to_string(),
            data,
        )
    }

    pub fn with_identity(
        event_id: impl Into<String>,
        occurred_at: impl Into<String>,
        event_type: impl Into<String>,
        data: Value,
    ) -> Self {
        Self {
            event_id: event_id.into(),
            occurred_at: occurred_at.into(),
            event_type: event_type.into(),
            data,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EventRecord {
    Full(FullEvent),
    Index(IndexEvent),
}

impl EventRecord {
    pub fn full(event: FullEvent) -> Self {
        EventRecord::Full(event)
    }

    pub fn index(event: IndexEvent) -> Self {
        EventRecord::Index(event)
    }

    pub fn stream(&self) -> EventStream {
        match self {
            EventRecord::Full(_) => EventStream::Full,
            EventRecord::Index(_) => EventStream::Index,
        }
    }

    pub fn event_id(&self) -> &str {
        match self {
            EventRecord::Full(event) => &event.event_id,
            EventRecord::Index(event) => &event.event_id,
        }
    }

    pub fn occurred_at(&self) -> &str {
        match self {
            EventRecord::Full(event) => &event.occurred_at,
            EventRecord::Index(event) => &event.occurred_at,
        }
    }

    pub fn event_type(&self) -> &str {
        match self {
            EventRecord::Full(event) => &event.event_type,
            EventRecord::Index(event) => &event.event_type,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EventWriter {
    repo_root: PathBuf,
}

impl EventWriter {
    pub fn new(repo_root: impl Into<PathBuf>) -> Self {
        Self {
            repo_root: repo_root.into(),
        }
    }

    pub fn write(&self, event: &EventRecord) -> Result<PathBuf, EventWriteError> {
        let rel_path = relative_path_for_event(
            event.stream(),
            event.occurred_at(),
            event.event_id(),
            event.event_type(),
        )?;
        let abs_path = self.repo_root.join(&rel_path);
        if let Some(parent) = abs_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&abs_path)?;
        serde_json::to_writer_pretty(&mut file, event)?;
        file.write_all(b"\n")?;
        file.sync_all()?;

        Ok(rel_path)
    }
}

pub fn relative_path_for_event(
    stream: EventStream,
    occurred_at: &str,
    event_id: &str,
    event_type: &str,
) -> Result<PathBuf, EventWriteError> {
    validate_filename_component("event_id", event_id)?;
    validate_filename_component("event_type", event_type)?;

    let timestamp = OffsetDateTime::parse(occurred_at, &Rfc3339).map_err(|source| {
        EventWriteError::InvalidTimestamp {
            value: occurred_at.to_string(),
            source,
        }
    })?;

    Ok(Path::new(stream.root_dir())
        .join(format!("{:04}", timestamp.year()))
        .join(format!("{:02}", u8::from(timestamp.month())))
        .join(format!("{:02}", timestamp.day()))
        .join(format!("{event_id}-{event_type}.json")))
}

pub fn new_event_id() -> String {
    Uuid::now_v7().to_string()
}

pub fn now_utc_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .expect("RFC3339 formatting for UTC timestamp should never fail")
}

fn validate_filename_component(field: &'static str, value: &str) -> Result<(), EventWriteError> {
    let is_valid = !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'));

    if is_valid {
        Ok(())
    } else {
        Err(EventWriteError::InvalidFileComponent {
            field,
            value: value.to_string(),
        })
    }
}

#[derive(Debug)]
pub enum EventWriteError {
    InvalidTimestamp {
        value: String,
        source: time::error::Parse,
    },
    InvalidFileComponent {
        field: &'static str,
        value: String,
    },
    Io(std::io::Error),
    Serialize(serde_json::Error),
}

impl fmt::Display for EventWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EventWriteError::InvalidTimestamp { value, source } => {
                write!(f, "invalid RFC3339 timestamp '{}': {}", value, source)
            }
            EventWriteError::InvalidFileComponent { field, value } => {
                write!(
                    f,
                    "invalid {} '{}': use only ASCII letters, numbers, '.', '-', '_'",
                    field, value
                )
            }
            EventWriteError::Io(err) => write!(f, "I/O error while writing event: {}", err),
            EventWriteError::Serialize(err) => {
                write!(f, "failed to serialize event as JSON: {}", err)
            }
        }
    }
}

impl Error for EventWriteError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            EventWriteError::InvalidTimestamp { source, .. } => Some(source),
            EventWriteError::Io(err) => Some(err),
            EventWriteError::Serialize(err) => Some(err),
            EventWriteError::InvalidFileComponent { .. } => None,
        }
    }
}

impl From<std::io::Error> for EventWriteError {
    fn from(value: std::io::Error) -> Self {
        EventWriteError::Io(value)
    }
}

impl From<serde_json::Error> for EventWriteError {
    fn from(value: serde_json::Error) -> Self {
        EventWriteError::Serialize(value)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        relative_path_for_event, EventRecord, EventStream, EventWriter, FullEvent, FullEventKind,
        IndexEvent, IndexEventKind,
    };
    use serde_json::json;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_tmp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before UNIX_EPOCH")
            .as_nanos();
        std::env::temp_dir().join(format!("knots-events-{}", nanos))
    }

    #[test]
    fn builds_deterministic_full_event_path() {
        let path = relative_path_for_event(
            EventStream::Full,
            "2026-02-22T17:00:00Z",
            "018f4f7f-7dc7-7f4e-954b-64f8a2273ec8",
            FullEventKind::KnotStateSet.as_str(),
        )
        .expect("path should build");
        assert_eq!(
            path.to_string_lossy(),
            ".knots/events/2026/02/22/018f4f7f-7dc7-7f4e-954b-64f8a2273ec8-knot.state_set.json"
        );
    }

    #[test]
    fn builds_deterministic_index_event_path() {
        let path = relative_path_for_event(
            EventStream::Index,
            "2026-02-22T17:00:00Z",
            "018f4f7f-7dc7-7f4e-954b-64f8a2273ec8",
            IndexEventKind::KnotHead.as_str(),
        )
        .expect("path should build");
        assert_eq!(
            path.to_string_lossy(),
            ".knots/index/2026/02/22/018f4f7f-7dc7-7f4e-954b-64f8a2273ec8-idx.knot_head.json"
        );
    }

    #[test]
    fn writes_append_only_full_event_file() {
        let root = unique_tmp_dir();
        let writer = EventWriter::new(&root);
        let event = EventRecord::full(FullEvent::with_identity(
            "018f4f7f-7dc7-7f4e-954b-64f8a2273ec8",
            "2026-02-22T17:00:00Z",
            "K-123",
            FullEventKind::KnotCreated.as_str(),
            json!({"title":"Build cache"}),
        ));

        let relative = writer.write(&event).expect("first write should succeed");
        assert_eq!(
            relative.to_string_lossy(),
            ".knots/events/2026/02/22/018f4f7f-7dc7-7f4e-954b-64f8a2273ec8-knot.created.json"
        );

        let absolute = root.join(&relative);
        let saved: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&absolute).expect("event JSON file should be readable"),
        )
        .expect("event JSON should parse");

        assert_eq!(saved["type"], "knot.created");
        assert_eq!(saved["knot_id"], "K-123");

        let second_write = writer.write(&event);
        assert!(second_write.is_err());
        if let Err(err) = second_write {
            assert!(
                err.to_string().contains("I/O error"),
                "expected create_new collision, got: {}",
                err
            );
        }

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_invalid_file_component() {
        let result = relative_path_for_event(
            EventStream::Full,
            "2026-02-22T17:00:00Z",
            "bad/id",
            "knot.created",
        );
        assert!(result.is_err());
    }

    #[test]
    fn writes_index_event() {
        let root = unique_tmp_dir();
        let writer = EventWriter::new(&root);
        let event = EventRecord::index(IndexEvent::with_identity(
            "018f4f7f-7dc7-7f4e-954b-64f8a2273ec8",
            "2026-02-22T17:00:00Z",
            IndexEventKind::KnotHead.as_str(),
            json!({
                "knot_id":"K-123",
                "title":"Build cache",
                "state":"implementing",
                "updated_at":"2026-02-22T17:00:00Z"
            }),
        ));

        let relative = writer.write(&event).expect("index write should succeed");
        assert_eq!(
            relative.to_string_lossy(),
            ".knots/index/2026/02/22/018f4f7f-7dc7-7f4e-954b-64f8a2273ec8-idx.knot_head.json"
        );

        let _ = std::fs::remove_dir_all(root);
    }
}
