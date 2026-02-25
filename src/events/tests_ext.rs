use std::error::Error;

use serde_json::json;

use super::{
    relative_path_for_event, EventRecord, EventStream, EventWriteError, FullEvent, FullEventKind,
    IndexEvent, IndexEventKind,
};

#[test]
fn full_event_kind_strings_cover_remaining_variants() {
    assert_eq!(FullEventKind::KnotBodySet.as_str(), "knot.body_set");
    assert_eq!(
        FullEventKind::KnotCommentAdded.as_str(),
        "knot.comment_added"
    );
    assert_eq!(
        FullEventKind::KnotReviewDecision.as_str(),
        "knot.review_decision"
    );
}

#[test]
fn new_event_builders_and_preconditions_set_expected_fields() {
    let full = FullEvent::new("K-1", FullEventKind::KnotCreated, json!({"title": "x"}))
        .with_precondition("etag-1");
    assert_eq!(full.knot_id, "K-1");
    assert_eq!(full.event_type, "knot.created");
    assert_eq!(
        full.precondition
            .as_ref()
            .map(|value| value.workflow_etag.as_str()),
        Some("etag-1")
    );

    let index = IndexEvent::new(IndexEventKind::KnotHead, json!({"knot_id": "K-1"}))
        .with_precondition("etag-2");
    assert_eq!(index.event_type, "idx.knot_head");
    assert_eq!(
        index
            .precondition
            .as_ref()
            .map(|value| value.workflow_etag.as_str()),
        Some("etag-2")
    );
}

#[test]
fn event_record_accessors_cover_full_and_index_variants() {
    let full = EventRecord::full(FullEvent::with_identity(
        "evt-full",
        "2026-02-25T10:00:00Z",
        "K-1",
        FullEventKind::KnotTypeSet.as_str(),
        json!({"type": "task"}),
    ));
    assert_eq!(full.stream(), EventStream::Full);
    assert_eq!(full.event_id(), "evt-full");
    assert_eq!(full.occurred_at(), "2026-02-25T10:00:00Z");
    assert_eq!(full.event_type(), "knot.type_set");

    let index = EventRecord::index(IndexEvent::with_identity(
        "evt-index",
        "2026-02-25T11:00:00Z",
        IndexEventKind::KnotHead.as_str(),
        json!({"knot_id": "K-1"}),
    ));
    assert_eq!(index.stream(), EventStream::Index);
    assert_eq!(index.event_id(), "evt-index");
    assert_eq!(index.occurred_at(), "2026-02-25T11:00:00Z");
    assert_eq!(index.event_type(), "idx.knot_head");
}

#[test]
fn relative_path_rejects_invalid_timestamp_values() {
    let result = relative_path_for_event(EventStream::Full, "not-rfc3339", "evt", "kind");
    assert!(matches!(
        result,
        Err(EventWriteError::InvalidTimestamp { .. })
    ));
}

#[test]
fn event_write_error_display_source_and_from_cover_variants() {
    let invalid_component = EventWriteError::InvalidFileComponent {
        field: "event_id",
        value: "bad/value".to_string(),
    };
    assert!(invalid_component
        .to_string()
        .contains("invalid event_id 'bad/value'"));
    assert!(invalid_component.source().is_none());

    let io_err: EventWriteError = std::io::Error::other("disk").into();
    assert!(io_err.to_string().contains("I/O error while writing event"));
    assert!(io_err.source().is_some());

    let serde_err =
        serde_json::from_slice::<serde_json::Value>(b"{").expect_err("invalid JSON should fail");
    let serialize = EventWriteError::Serialize(serde_err);
    assert!(serialize
        .to_string()
        .contains("failed to serialize event as JSON"));
    assert!(serialize.source().is_some());

    let converted: EventWriteError = serde_json::from_slice::<serde_json::Value>(b"{")
        .expect_err("invalid JSON should fail")
        .into();
    assert!(matches!(converted, EventWriteError::Serialize(_)));
}
