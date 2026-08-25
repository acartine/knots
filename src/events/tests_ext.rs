use std::error::Error;

use serde_json::json;

use super::{
    relative_path_for_event, EventRecord, EventStream, EventWriteError, EventWriter, FullEvent,
    FullEventKind, IndexEvent, IndexEventKind,
};
use crate::domain::scope::{ScopeData, ScopeFloat};

#[test]
fn durable_receipt_precedes_compatibility_file_creation() {
    let workspace = knots_test_support::workspace("knots-event-outbox");
    let root = workspace.path().join(".knots");
    let db_path = root.join("cache/state.sqlite");
    std::fs::create_dir_all(db_path.parent().expect("cache parent"))
        .expect("create cache directory");
    crate::db::open_connection(&db_path.to_string_lossy()).expect("migrate database");
    let writer = EventWriter::with_db_path(&root, &db_path);
    let event = EventRecord::full(FullEvent::with_identity(
        "018f4f7f-7dc7-7f4e-954b-64f8a2273ec8",
        "2026-02-22T17:00:00Z",
        "K-123",
        FullEventKind::KnotCreated.as_str(),
        json!({"title":"Build cache"}),
    ));
    let relative = relative_path_for_event(
        event.stream(),
        event.occurred_at(),
        event.event_id(),
        event.event_type(),
    )
    .expect("event path");
    let absolute = root.join(relative);
    std::fs::create_dir_all(absolute.parent().expect("event parent"))
        .expect("create event directory");
    std::fs::write(&absolute, b"collision").expect("seed collision");

    assert!(writer.write(&event).is_err());
    let conn = crate::db::open_connection(&db_path.to_string_lossy()).expect("open database");
    let receipts: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM v2_outbox WHERE event_id = ?1",
            [event.event_id()],
            |row| row.get(0),
        )
        .expect("count receipt");
    assert_eq!(receipts, 1);
    assert_eq!(
        std::fs::read(absolute).expect("read collision"),
        b"collision"
    );
}

#[test]
fn full_event_kind_strings_cover_remaining_variants() {
    assert_eq!(FullEventKind::KnotBodySet.as_str(), "knot.body_set");
    assert_eq!(
        FullEventKind::KnotCommentAdded.as_str(),
        "knot.comment_added"
    );
    assert_eq!(
        FullEventKind::KnotInvariantsSet.as_str(),
        "knot.invariants_set"
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
            .map(|value| value.profile_etag.as_str()),
        Some("etag-1")
    );

    let index = IndexEvent::new(IndexEventKind::KnotHead, json!({"knot_id": "K-1"}))
        .with_precondition("etag-2");
    assert_eq!(index.event_type, "idx.knot_head");
    assert_eq!(
        index
            .precondition
            .as_ref()
            .map(|value| value.profile_etag.as_str()),
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
fn knot_scope_set_payload_uses_spec_keys() {
    let scope = ScopeData {
        volume: Some(8),
        scale: Some("fib_v1".to_string()),
        volume_score_confidence: Some(ScopeFloat::new(0.72).expect("finite")),
        volume_stddev: Some(ScopeFloat::new(1.25).expect("finite")),
        volume_result_id: Some("vol-1".to_string()),
        reliability: Some(44),
        reliability_score_confidence: Some(ScopeFloat::new(0.91).expect("finite")),
        reliability_stddev: Some(ScopeFloat::new(2.5).expect("finite")),
        reliability_band: Some("medium".to_string()),
        reliability_result_id: Some("rel-1".to_string()),
    };
    let payload = serde_json::to_value(&scope).expect("scope serializes");
    let object = payload.as_object().expect("scope payload is an object");

    for key in [
        "volume",
        "scale",
        "volume_score_confidence",
        "volume_stddev",
        "volume_result_id",
        "reliability",
        "reliability_score_confidence",
        "reliability_stddev",
        "reliability_band",
        "reliability_result_id",
    ] {
        assert!(
            object.contains_key(key),
            "payload should contain spec field `{key}`"
        );
    }
    assert_eq!(object.len(), 10, "payload should not contain extra keys");
    let event = FullEvent::new("K-1", FullEventKind::KnotScopeSet, payload);
    assert_eq!(event.event_type, "knot.scope_set");
}

#[test]
fn knot_scope_set_payload_omits_absent_fields() {
    let scope = ScopeData {
        volume: Some(3),
        reliability_band: Some("high".to_string()),
        ..ScopeData::default()
    };
    let payload = serde_json::to_value(&scope).expect("scope serializes");
    let object = payload.as_object().expect("scope payload is an object");

    assert!(object.contains_key("volume"));
    assert!(object.contains_key("reliability_band"));
    for absent in [
        "scale",
        "volume_score_confidence",
        "volume_stddev",
        "volume_result_id",
        "reliability",
        "reliability_score_confidence",
        "reliability_stddev",
        "reliability_result_id",
    ] {
        assert!(
            !object.contains_key(absent),
            "absent field `{absent}` should be skipped in JSON"
        );
    }
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
