use std::error::Error;
use std::path::PathBuf;

use uuid::Uuid;

use super::{run_fsck, FsckError};

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-fsck-ext-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    root
}

fn write_file(path: &PathBuf, content: &str) {
    std::fs::create_dir_all(path.parent().expect("parent should exist"))
        .expect("parent should be creatable");
    std::fs::write(path, content).expect("fixture file should be writable");
}

#[test]
fn fsck_error_display_source_and_from_cover_io_variant() {
    let err: FsckError = std::io::Error::other("disk").into();
    assert!(err.to_string().contains("I/O error"));
    assert!(err.source().is_some());
}

#[test]
fn reports_schema_and_reference_issues_for_malformed_events() {
    let root = unique_workspace();

    let invalid_json = root.join(".knots/events/2026/02/25/1000-knot.created.json");
    write_file(&invalid_json, "{");

    let non_object = root.join(".knots/events/2026/02/25/1001-knot.created.json");
    write_file(&non_object, "[]\n");

    let missing_event_id = root.join(".knots/events/2026/02/25/1002-knot.created.json");
    write_file(
        &missing_event_id,
        concat!(
            "{\n",
            "  \"occurred_at\": \"2026-02-25T10:00:00Z\",\n",
            "  \"type\": \"knot.created\",\n",
            "  \"knot_id\": \"K-a\",\n",
            "  \"data\": {}\n",
            "}\n"
        ),
    );

    let missing_required_fields = root.join(".knots/events/2026/02/25/1003-knot.created.json");
    write_file(
        &missing_required_fields,
        concat!(
            "{\n",
            "  \"event_id\": \"1003\",\n",
            "  \"occurred_at\": \"\",\n",
            "  \"type\": \"\",\n",
            "  \"knot_id\": \"K-b\",\n",
            "  \"data\": {}\n",
            "}\n"
        ),
    );

    let filename_mismatch = root.join(".knots/events/2026/02/25/not-expected-name.json");
    write_file(
        &filename_mismatch,
        concat!(
            "{\n",
            "  \"event_id\": \"1004\",\n",
            "  \"occurred_at\": \"2026-02-25T10:00:00Z\",\n",
            "  \"type\": \"knot.created\",\n",
            "  \"knot_id\": \"K-c\",\n",
            "  \"data\": {}\n",
            "}\n"
        ),
    );

    let missing_data_object = root.join(".knots/events/2026/02/25/1005-knot.created.json");
    write_file(
        &missing_data_object,
        concat!(
            "{\n",
            "  \"event_id\": \"1005\",\n",
            "  \"occurred_at\": \"2026-02-25T10:00:00Z\",\n",
            "  \"type\": \"knot.created\",\n",
            "  \"knot_id\": \"K-d\",\n",
            "  \"data\": \"bad\"\n",
            "}\n"
        ),
    );

    let missing_knot_id = root.join(".knots/events/2026/02/25/1006-knot.created.json");
    write_file(
        &missing_knot_id,
        concat!(
            "{\n",
            "  \"event_id\": \"1006\",\n",
            "  \"occurred_at\": \"2026-02-25T10:00:00Z\",\n",
            "  \"type\": \"knot.created\",\n",
            "  \"data\": {}\n",
            "}\n"
        ),
    );

    let bad_edge_data = root.join(".knots/events/2026/02/25/1007-knot.edge_add.json");
    write_file(
        &bad_edge_data,
        concat!(
            "{\n",
            "  \"event_id\": \"1007\",\n",
            "  \"occurred_at\": \"2026-02-25T10:00:00Z\",\n",
            "  \"type\": \"knot.edge_add\",\n",
            "  \"knot_id\": \"K-src\",\n",
            "  \"data\": {}\n",
            "}\n"
        ),
    );

    let valid_index = root.join(".knots/index/2026/02/25/2000-idx.knot_head.json");
    write_file(
        &valid_index,
        concat!(
            "{\n",
            "  \"event_id\": \"2000\",\n",
            "  \"occurred_at\": \"2026-02-25T10:00:00Z\",\n",
            "  \"type\": \"idx.knot_head\",\n",
            "  \"data\": {\n",
            "    \"knot_id\": \"K-src\",\n",
            "    \"title\": \"Source\",\n",
            "    \"state\": \"work_item\",\n",
            "    \"updated_at\": \"2026-02-25T10:00:00Z\"\n",
            "  }\n",
            "}\n"
        ),
    );

    let edge_with_missing_destination =
        root.join(".knots/events/2026/02/25/2001-knot.edge_add.json");
    write_file(
        &edge_with_missing_destination,
        concat!(
            "{\n",
            "  \"event_id\": \"2001\",\n",
            "  \"occurred_at\": \"2026-02-25T10:00:01Z\",\n",
            "  \"type\": \"knot.edge_add\",\n",
            "  \"knot_id\": \"K-src\",\n",
            "  \"data\": {\n",
            "    \"kind\": \"blocked_by\",\n",
            "    \"dst\": \"K-missing\"\n",
            "  }\n",
            "}\n"
        ),
    );

    let index_missing_fields = root.join(".knots/index/2026/02/25/2002-idx.knot_head.json");
    write_file(
        &index_missing_fields,
        concat!(
            "{\n",
            "  \"event_id\": \"2002\",\n",
            "  \"occurred_at\": \"2026-02-25T10:00:00Z\",\n",
            "  \"type\": \"idx.knot_head\",\n",
            "  \"data\": {\n",
            "    \"knot_id\": \"K-partial\"\n",
            "  }\n",
            "}\n"
        ),
    );

    let report = run_fsck(&root).expect("fsck should complete");
    assert!(!report.ok());
    assert!(report.files_scanned >= 10);

    let messages = report
        .issues
        .iter()
        .map(|issue| issue.message.as_str())
        .collect::<Vec<_>>();
    assert!(messages.iter().any(|m| m.contains("invalid JSON payload")));
    assert!(messages
        .iter()
        .any(|m| m.contains("event payload must be a JSON object")));
    assert!(messages
        .iter()
        .any(|m| m.contains("missing required string field 'event_id'")));
    assert!(messages
        .iter()
        .any(|m| m.contains("missing required string field 'occurred_at'")));
    assert!(messages
        .iter()
        .any(|m| m.contains("missing required string field 'type'")));
    assert!(messages
        .iter()
        .any(|m| m.contains("event filename mismatch")));
    assert!(messages
        .iter()
        .any(|m| m.contains("missing required object field 'data'")));
    assert!(messages
        .iter()
        .any(|m| m.contains("missing required string field 'knot_id'")));
    assert!(messages
        .iter()
        .any(|m| m.contains("missing required string field data.dst")));
    assert!(messages
        .iter()
        .any(|m| m.contains("missing required string field data.kind")));
    assert!(messages
        .iter()
        .any(|m| m.contains("missing required string field data.title")));
    assert!(messages
        .iter()
        .any(|m| m.contains("missing required string field data.state")));
    assert!(messages
        .iter()
        .any(|m| m.contains("missing required string field data.updated_at")));
    assert!(messages
        .iter()
        .any(|m| m.contains("edge destination 'K-missing'")));

    let _ = std::fs::remove_dir_all(root);
}
