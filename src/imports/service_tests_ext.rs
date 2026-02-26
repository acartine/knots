use std::path::{Path, PathBuf};

use crate::db;
use crate::events::EventWriter;

use super::{
    collect_handoff_capsules, collect_notes, collect_tags, dedupe_stable, deterministic_entry_id,
    fingerprint, metadata_from_legacy_text, metadata_from_source_entry, normalize_non_empty,
    ImportService, SourceIssue, SourceMetadataEntry, SourceNotesField,
};

fn unique_workspace() -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("knots-import-service-ext-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    root
}

fn base_issue() -> SourceIssue {
    SourceIssue {
        id: "ISS-1".to_string(),
        title: "Imported issue".to_string(),
        profile_id: None,
        workflow_id: Some("default".to_string()),
        description: None,
        body: None,
        notes: None,
        handoff_capsules: Vec::new(),
        state: None,
        status: Some("open".to_string()),
        priority: None,
        owner: Some("owner".to_string()),
        created_by: Some("creator".to_string()),
        issue_type: None,
        type_name: None,
        labels: Vec::new(),
        tags: Vec::new(),
        dependencies: Vec::new(),
        created_at: Some("2026-02-25T10:00:00Z".to_string()),
        updated_at: Some("2026-02-25T10:01:00Z".to_string()),
        closed_at: None,
        close_reason: None,
    }
}

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent directory should be creatable");
    }
    std::fs::write(path, contents).expect("file should be writable");
}

#[test]
fn helper_functions_cover_normalization_dedupe_and_metadata_paths() {
    assert!(normalize_non_empty(Some("   ")).is_none());
    assert_eq!(
        normalize_non_empty(Some(" value ")).as_deref(),
        Some("value")
    );

    let token_a = fingerprint("jsonl:/tmp/file", "ISS-1", "2026-02-25T10:00:00Z", "upsert");
    let token_b = fingerprint("jsonl:/tmp/file", "ISS-1", "2026-02-25T10:00:00Z", "upsert");
    assert_eq!(token_a, token_b);

    let mut values = vec![
        "one".to_string(),
        "two".to_string(),
        "one".to_string(),
        "two".to_string(),
        "three".to_string(),
    ];
    dedupe_stable(&mut values);
    assert_eq!(values, vec!["one", "two", "three"]);

    let mut issue = base_issue();
    issue.labels = vec!["Alpha".to_string(), " ".to_string()];
    issue.tags = vec!["beta".to_string(), "alpha".to_string()];
    let tags = collect_tags(&issue);
    assert_eq!(tags, vec!["alpha".to_string(), "beta".to_string()]);

    let legacy = metadata_from_legacy_text(
        "jsonl:/tmp/issues.jsonl",
        &issue,
        "ISS-1",
        "notes",
        "legacy note",
        0,
        "2026-02-25T10:00:00Z",
    )
    .expect("legacy metadata should be produced");
    assert_eq!(legacy.content, "legacy note");

    let mapped_none = metadata_from_source_entry(
        "jsonl:/tmp/issues.jsonl",
        &issue,
        "ISS-1",
        "notes",
        &SourceMetadataEntry {
            entry_id: None,
            content: Some("  ".to_string()),
            username: None,
            datetime: None,
            agentname: None,
            model: None,
            version: None,
        },
        0,
        "2026-02-25T10:00:00Z",
        "2026-02-25T10:01:00Z",
    )
    .expect("empty content metadata should not fail");
    assert!(mapped_none.is_none());

    let mapped = metadata_from_source_entry(
        "jsonl:/tmp/issues.jsonl",
        &issue,
        "ISS-1",
        "notes",
        &SourceMetadataEntry {
            entry_id: None,
            content: Some("entry content".to_string()),
            username: Some("alice".to_string()),
            datetime: Some("2026-02-25T10:05:00Z".to_string()),
            agentname: Some("agent".to_string()),
            model: Some("model".to_string()),
            version: Some("v1".to_string()),
        },
        1,
        "2026-02-25T10:00:00Z",
        "2026-02-25T10:01:00Z",
    )
    .expect("entry metadata should map")
    .expect("entry metadata should exist");
    assert_eq!(mapped.username, "alice");

    let generated = deterministic_entry_id(
        "jsonl:/tmp/issues.jsonl",
        "ISS-1",
        "notes",
        2,
        "text",
        "alice",
        "2026-02-25T10:05:00Z",
        "agent",
        "model",
        "v1",
    );
    assert_eq!(generated.len(), 64);
}

#[test]
fn metadata_collection_handles_text_and_entry_variants() {
    let mut issue = base_issue();
    issue.notes = Some(SourceNotesField::Text("legacy".to_string()));
    issue.handoff_capsules.push(SourceMetadataEntry {
        entry_id: None,
        content: Some("handoff".to_string()),
        username: Some("u".to_string()),
        datetime: Some("2026-02-25T10:10:00Z".to_string()),
        agentname: Some("a".to_string()),
        model: Some("m".to_string()),
        version: Some("v".to_string()),
    });

    let notes = collect_notes(
        "jsonl:/tmp/issues.jsonl",
        &issue,
        "ISS-1",
        "2026-02-25T10:00:00Z",
        "2026-02-25T10:01:00Z",
    )
    .expect("notes should map");
    assert_eq!(notes.len(), 1);

    let handoff = collect_handoff_capsules(
        "jsonl:/tmp/issues.jsonl",
        &issue,
        "ISS-1",
        "2026-02-25T10:00:00Z",
        "2026-02-25T10:01:00Z",
    )
    .expect("handoff capsules should map");
    assert_eq!(handoff.len(), 1);
}

#[test]
fn import_jsonl_covers_partial_checkpoint_and_dry_run_statuses() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(
        db_path
            .parent()
            .expect("db parent directory should exist for import test"),
    )
    .expect("db parent should be creatable");
    let conn = db::open_connection(db_path.to_str().expect("utf8 path")).expect("db should open");
    let writer = EventWriter::new(root.clone());
    let service = ImportService::new(&conn, &writer);

    let input = root.join("issues.jsonl");
    write_file(
        &input,
        concat!(
            "{\"id\":\"ISS-1\",\"title\":\"Imported\",\"workflow_id\":\"default\",",
            "\"description\":\"desc\",\"body\":\"body\",\"status\":\"open\",",
            "\"priority\":2,\"type\":\"task\",\"labels\":[\"alpha\"],",
            "\"tags\":[\"beta\"],\"dependencies\":[{\"depends_on_id\":\"ISS-2\",",
            "\"type\":\"blocks\"}],\"notes\":[{\"content\":\"note\"}],",
            "\"handoff_capsules\":[{\"content\":\"handoff\"}],",
            "\"updated_at\":\"2026-02-25T10:01:00Z\",",
            "\"created_at\":\"2026-02-25T10:00:00Z\"}\n",
            "{this is not json}\n",
            "{\"id\":\"ISS-2\",\"title\":\"\",\"workflow_id\":\"default\",",
            "\"status\":\"open\",\"updated_at\":\"2026-02-25T10:02:00Z\"}\n"
        ),
    );

    let summary = service
        .import_jsonl(input.to_str().expect("utf8 path"), None, false)
        .expect("import should complete with partial status");
    assert_eq!(summary.status, "partial");
    assert_eq!(summary.imported_count, 1);
    assert_eq!(summary.error_count, 2);
    assert!(summary.last_error.is_some());
    assert_eq!(summary.checkpoint.as_deref(), Some("3"));

    let second = service
        .import_jsonl(input.to_str().expect("utf8 path"), None, false)
        .expect("second import should use checkpoint and skip lines");
    assert_eq!(second.processed_count, 0);
    assert_eq!(second.imported_count, 0);

    let dry_run_input = root.join("issues-dry-run.jsonl");
    write_file(
        &dry_run_input,
        concat!(
            "{\"id\":\"ISS-3\",\"title\":\"Dry run\",\"workflow_id\":\"default\",",
            "\"status\":\"open\",\"updated_at\":\"2026-02-25T10:03:00Z\"}\n"
        ),
    );
    let dry_summary = service
        .import_jsonl(dry_run_input.to_str().expect("utf8 path"), None, true)
        .expect("dry-run import should succeed");
    assert_eq!(dry_summary.status, "dry_run");
    assert_eq!(dry_summary.imported_count, 1);

    let statuses = service
        .list_statuses()
        .expect("status listing should succeed");
    assert!(statuses.iter().any(|status| status.source_type == "jsonl"));

    let _ = std::fs::remove_dir_all(root);
}
