use std::fs::File;
use std::io::{BufRead, BufReader};

use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::db::{self, UpsertKnotHot};
use crate::domain::metadata::{normalize_datetime, normalize_text, MetadataEntry};
use crate::events::{
    new_event_id, now_utc_rfc3339, EventRecord, EventWriter, FullEvent, FullEventKind, IndexEvent,
    IndexEventKind,
};
use crate::workflow::normalize_workflow_id;

use super::errors::ImportError;
use super::source::{
    map_dependency_kind, map_source_state, merged_body, normalize_path, parse_since,
    parse_timestamp, source_key, SourceIssue, SourceKind, SourceMetadataEntry, SourceNotesField,
};
use super::store::{fingerprint_exists, insert_fingerprint, load_checkpoint};

pub struct ImportService<'a> {
    conn: &'a Connection,
    writer: &'a EventWriter,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ImportSummary {
    pub source_type: String,
    pub source_ref: String,
    pub status: String,
    pub processed_count: u64,
    pub imported_count: u64,
    pub skipped_count: u64,
    pub error_count: u64,
    pub checkpoint: Option<String>,
    pub last_error: Option<String>,
    pub dry_run: bool,
    pub last_run_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ImportStatus {
    pub source_type: String,
    pub source_ref: String,
    pub status: String,
    pub processed_count: u64,
    pub imported_count: u64,
    pub skipped_count: u64,
    pub error_count: u64,
    pub checkpoint: Option<String>,
    pub last_error: Option<String>,
    pub last_run_at: String,
}

#[derive(Debug, Clone)]
struct ImportRun {
    processed_count: u64,
    imported_count: u64,
    skipped_count: u64,
    error_count: u64,
    checkpoint: Option<String>,
    last_error: Option<String>,
}

impl ImportRun {
    fn new() -> Self {
        Self {
            processed_count: 0,
            imported_count: 0,
            skipped_count: 0,
            error_count: 0,
            checkpoint: None,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Imported,
    Skipped,
}

impl<'a> ImportService<'a> {
    pub fn new(conn: &'a Connection, writer: &'a EventWriter) -> Self {
        Self { conn, writer }
    }

    pub fn import_jsonl(
        &self,
        file: &str,
        since: Option<&str>,
        dry_run: bool,
    ) -> Result<ImportSummary, ImportError> {
        let source_ref = normalize_path(file)?;
        let source_key = source_key(SourceKind::Jsonl, &source_ref);
        let since_ts = parse_since(since)?;
        let previous_checkpoint = load_checkpoint(self.conn, &source_key)?;

        let handle = File::open(&source_ref)?;
        let reader = BufReader::new(handle);
        let mut run = ImportRun::new();

        for (index, line) in reader.lines().enumerate() {
            let line_number = index + 1;
            run.checkpoint = Some(line_number.to_string());
            if let Some(cp) = previous_checkpoint {
                if line_number <= cp {
                    continue;
                }
            }

            run.processed_count += 1;
            let text = line.map_err(ImportError::Io)?;
            let issue: SourceIssue = match serde_json::from_str(&text) {
                Ok(issue) => issue,
                Err(err) => {
                    run.error_count += 1;
                    run.last_error = Some(format!("line {}: invalid JSON: {}", line_number, err));
                    continue;
                }
            };

            match self.import_issue(&source_ref, &source_key, issue, since_ts, dry_run) {
                Ok(Outcome::Imported) => run.imported_count += 1,
                Ok(Outcome::Skipped) => run.skipped_count += 1,
                Err(ImportError::InvalidRecord(message)) => {
                    run.error_count += 1;
                    run.last_error = Some(format!("line {}: {}", line_number, message));
                }
                Err(err) => {
                    run.last_error = Some(format!("line {}: {}", line_number, err));
                    return self.finish_run(
                        SourceKind::Jsonl,
                        source_ref,
                        source_key,
                        run,
                        "failed",
                        dry_run,
                        Err(err),
                    );
                }
            }
        }

        let status = if dry_run {
            "dry_run"
        } else if run.error_count > 0 {
            "partial"
        } else {
            "completed"
        };
        self.finish_run(
            SourceKind::Jsonl,
            source_ref,
            source_key,
            run,
            status,
            dry_run,
            Ok(()),
        )
    }

    pub fn list_statuses(&self) -> Result<Vec<ImportStatus>, ImportError> {
        let mut stmt = self.conn.prepare(
            r#"
SELECT source_type, source_ref, last_status, processed_count, imported_count,
       skipped_count, error_count, checkpoint, last_error, last_run_at
FROM import_state
ORDER BY last_run_at DESC, source_type ASC
"#,
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(ImportStatus {
                source_type: row.get(0)?,
                source_ref: row.get(1)?,
                status: row.get(2)?,
                processed_count: row.get::<_, i64>(3)? as u64,
                imported_count: row.get::<_, i64>(4)? as u64,
                skipped_count: row.get::<_, i64>(5)? as u64,
                error_count: row.get::<_, i64>(6)? as u64,
                checkpoint: row.get(7)?,
                last_error: row.get(8)?,
                last_run_at: row.get(9)?,
            });
        }
        Ok(out)
    }

    fn import_issue(
        &self,
        source_ref: &str,
        source_key: &str,
        issue: SourceIssue,
        since_ts: Option<OffsetDateTime>,
        dry_run: bool,
    ) -> Result<Outcome, ImportError> {
        if issue.id.trim().is_empty() || issue.title.trim().is_empty() {
            return Err(ImportError::InvalidRecord(
                "record requires non-empty id and title".to_string(),
            ));
        }

        let issue_id = issue.id.trim().to_string();
        let issue_title = issue.title.trim().to_string();
        let created_at =
            parse_timestamp(issue.created_at.as_deref().or(issue.updated_at.as_deref()))
                .unwrap_or_else(now_utc_rfc3339);
        let updated_at = parse_timestamp(
            issue
                .updated_at
                .as_deref()
                .or(issue.closed_at.as_deref())
                .or(issue.created_at.as_deref()),
        )
        .unwrap_or_else(|| created_at.clone());
        let updated_ts = OffsetDateTime::parse(&updated_at, &Rfc3339)
            .map_err(|_| ImportError::InvalidRecord("invalid updated_at timestamp".to_string()))?;
        if let Some(since_limit) = since_ts {
            if updated_ts < since_limit {
                return Ok(Outcome::Skipped);
            }
        }

        let action = "issue_upsert";
        let token = fingerprint(source_key, &issue_id, &updated_at, action);
        if fingerprint_exists(self.conn, &token)? {
            return Ok(Outcome::Skipped);
        }

        if dry_run {
            return Ok(Outcome::Imported);
        }

        let state = map_source_state(&issue)?;
        let raw_workflow_id = normalize_non_empty(issue.workflow_id.as_deref())
            .ok_or_else(|| ImportError::InvalidRecord("workflow_id is required".to_string()))?;
        let workflow_id = normalize_workflow_id(&raw_workflow_id)
            .ok_or_else(|| ImportError::InvalidRecord("workflow_id is required".to_string()))?;
        let description = merged_body(&issue);
        let body = description.clone();
        let knot_type =
            normalize_non_empty(issue.type_name.as_deref().or(issue.issue_type.as_deref()));
        let mut tags = collect_tags(&issue);
        let source_tag = format!("source:{}", source_ref);
        tags.push(source_tag.clone());
        dedupe_stable(&mut tags);

        let notes = collect_notes(source_key, &issue, &issue_id, &created_at, &updated_at)?;
        let handoff_capsules =
            collect_handoff_capsules(source_key, &issue, &issue_id, &created_at, &updated_at)?;

        let created_event = FullEvent::with_identity(
            new_event_id(),
            created_at.clone(),
            issue_id.clone(),
            FullEventKind::KnotCreated.as_str(),
            json!({
                "title": issue_title,
                "state": state.as_str(),
                "workflow_id": &workflow_id,
                "body": body,
                "description": description,
                "source": source_tag,
            }),
        );
        self.writer.write(&EventRecord::full(created_event))?;

        if let Some(description) = description.as_deref() {
            let event = FullEvent::with_identity(
                new_event_id(),
                created_at.clone(),
                issue_id.clone(),
                FullEventKind::KnotDescriptionSet.as_str(),
                json!({ "description": description }),
            );
            self.writer.write(&EventRecord::full(event))?;
        }

        if let Some(priority) = issue.priority {
            let event = FullEvent::with_identity(
                new_event_id(),
                created_at.clone(),
                issue_id.clone(),
                FullEventKind::KnotPrioritySet.as_str(),
                json!({ "priority": priority }),
            );
            self.writer.write(&EventRecord::full(event))?;
        }

        if let Some(knot_type) = knot_type.as_deref() {
            let event = FullEvent::with_identity(
                new_event_id(),
                created_at.clone(),
                issue_id.clone(),
                FullEventKind::KnotTypeSet.as_str(),
                json!({ "type": knot_type }),
            );
            self.writer.write(&EventRecord::full(event))?;
        }

        for tag in &tags {
            let event = FullEvent::with_identity(
                new_event_id(),
                created_at.clone(),
                issue_id.clone(),
                FullEventKind::KnotTagAdd.as_str(),
                json!({ "tag": tag }),
            );
            self.writer.write(&EventRecord::full(event))?;
        }

        for dependency in &issue.dependencies {
            if let Some(depends_on) = dependency.depends_on_id.as_deref() {
                let kind = map_dependency_kind(dependency.dep_type.as_deref());
                let edge_event = FullEvent::with_identity(
                    new_event_id(),
                    created_at.clone(),
                    issue_id.clone(),
                    FullEventKind::KnotEdgeAdd.as_str(),
                    json!({"kind": kind, "dst": depends_on}),
                );
                self.writer.write(&EventRecord::full(edge_event))?;
                self.conn.execute(
                    "INSERT OR IGNORE INTO edge (src, kind, dst) VALUES (?1, ?2, ?3)",
                    params![&issue_id, kind, depends_on],
                )?;
            }
        }

        for note in &notes {
            let event = FullEvent::with_identity(
                new_event_id(),
                note.datetime.clone(),
                issue_id.clone(),
                FullEventKind::KnotNoteAdded.as_str(),
                json!({
                    "entry_id": note.entry_id,
                    "content": note.content,
                    "username": note.username,
                    "datetime": note.datetime,
                    "agentname": note.agentname,
                    "model": note.model,
                    "version": note.version,
                }),
            );
            self.writer.write(&EventRecord::full(event))?;
        }

        for capsule in &handoff_capsules {
            let event = FullEvent::with_identity(
                new_event_id(),
                capsule.datetime.clone(),
                issue_id.clone(),
                FullEventKind::KnotHandoffCapsuleAdded.as_str(),
                json!({
                    "entry_id": capsule.entry_id,
                    "content": capsule.content,
                    "username": capsule.username,
                    "datetime": capsule.datetime,
                    "agentname": capsule.agentname,
                    "model": capsule.model,
                    "version": capsule.version,
                }),
            );
            self.writer.write(&EventRecord::full(event))?;
        }

        if let Some(reason) = issue.close_reason.as_deref() {
            let comment_event = FullEvent::with_identity(
                new_event_id(),
                updated_at.clone(),
                issue_id.clone(),
                FullEventKind::KnotCommentAdded.as_str(),
                json!({ "comment": reason }),
            );
            self.writer.write(&EventRecord::full(comment_event))?;
        }

        let index_event_id = new_event_id();
        let index_event = IndexEvent::with_identity(
            index_event_id.clone(),
            updated_at.clone(),
            IndexEventKind::KnotHead.as_str(),
            json!({
                "knot_id": &issue_id,
                "title": &issue_title,
                "state": state.as_str(),
                "workflow_id": &workflow_id,
                "updated_at": updated_at,
                "terminal": state.is_terminal(),
            }),
        );
        self.writer.write(&EventRecord::index(index_event))?;

        db::upsert_knot_hot(
            self.conn,
            &UpsertKnotHot {
                id: &issue_id,
                title: &issue_title,
                state: state.as_str(),
                updated_at: &updated_at,
                body: body.as_deref(),
                description: description.as_deref(),
                priority: issue.priority,
                knot_type: knot_type.as_deref(),
                tags: &tags,
                notes: &notes,
                handoff_capsules: &handoff_capsules,
                workflow_id: &workflow_id,
                workflow_etag: Some(&index_event_id),
                created_at: Some(&created_at),
            },
        )?;

        insert_fingerprint(
            self.conn,
            &token,
            source_key,
            &issue_id,
            &updated_at,
            action,
        )?;
        Ok(Outcome::Imported)
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_run(
        &self,
        source_kind: SourceKind,
        source_ref: String,
        source_key: String,
        run: ImportRun,
        status: &str,
        dry_run: bool,
        result: Result<(), ImportError>,
    ) -> Result<ImportSummary, ImportError> {
        let last_run_at = now_utc_rfc3339();
        self.conn.execute(
            r#"
INSERT INTO import_state (
    source_key, source_type, source_ref, last_run_at, last_status,
    processed_count, imported_count, skipped_count, error_count, checkpoint, last_error
)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
ON CONFLICT(source_key) DO UPDATE SET
    source_type = excluded.source_type,
    source_ref = excluded.source_ref,
    last_run_at = excluded.last_run_at,
    last_status = excluded.last_status,
    processed_count = excluded.processed_count,
    imported_count = excluded.imported_count,
    skipped_count = excluded.skipped_count,
    error_count = excluded.error_count,
    checkpoint = excluded.checkpoint,
    last_error = excluded.last_error
"#,
            params![
                source_key,
                source_kind.as_str(),
                source_ref,
                last_run_at,
                status,
                run.processed_count as i64,
                run.imported_count as i64,
                run.skipped_count as i64,
                run.error_count as i64,
                run.checkpoint,
                run.last_error
            ],
        )?;

        result?;
        Ok(ImportSummary {
            source_type: source_kind.as_str().to_string(),
            source_ref,
            status: status.to_string(),
            processed_count: run.processed_count,
            imported_count: run.imported_count,
            skipped_count: run.skipped_count,
            error_count: run.error_count,
            checkpoint: run.checkpoint,
            last_error: run.last_error,
            dry_run,
            last_run_at,
        })
    }
}

fn fingerprint(source_key: &str, knot_id: &str, occurred_at: &str, action: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source_key.as_bytes());
    hasher.update(b"|");
    hasher.update(knot_id.as_bytes());
    hasher.update(b"|");
    hasher.update(occurred_at.as_bytes());
    hasher.update(b"|");
    hasher.update(action.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{:02x}", byte);
    }
    out
}

fn normalize_non_empty(raw: Option<&str>) -> Option<String> {
    let value = raw?.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn dedupe_stable(values: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    values.retain(|item| seen.insert(item.clone()));
}

fn collect_tags(issue: &SourceIssue) -> Vec<String> {
    let mut out = Vec::new();
    out.extend(
        issue
            .labels
            .iter()
            .map(String::as_str)
            .filter_map(|value| normalize_non_empty(Some(value)))
            .map(|value| value.to_ascii_lowercase()),
    );
    out.extend(
        issue
            .tags
            .iter()
            .map(String::as_str)
            .filter_map(|value| normalize_non_empty(Some(value)))
            .map(|value| value.to_ascii_lowercase()),
    );
    dedupe_stable(&mut out);
    out
}

fn collect_notes(
    source_key: &str,
    issue: &SourceIssue,
    issue_id: &str,
    created_at: &str,
    updated_at: &str,
) -> Result<Vec<MetadataEntry>, ImportError> {
    let mut out = Vec::new();
    match issue.notes.as_ref() {
        Some(SourceNotesField::Text(text)) => {
            if let Some(entry) =
                metadata_from_legacy_text(source_key, issue, issue_id, "notes", text, 0, updated_at)
            {
                out.push(entry);
            }
        }
        Some(SourceNotesField::Entries(entries)) => {
            for (idx, entry) in entries.iter().enumerate() {
                if let Some(mapped) = metadata_from_source_entry(
                    source_key, issue, issue_id, "notes", entry, idx, created_at, updated_at,
                )? {
                    out.push(mapped);
                }
            }
        }
        None => {}
    }
    Ok(out)
}

fn collect_handoff_capsules(
    source_key: &str,
    issue: &SourceIssue,
    issue_id: &str,
    created_at: &str,
    updated_at: &str,
) -> Result<Vec<MetadataEntry>, ImportError> {
    let mut out = Vec::new();
    for (idx, entry) in issue.handoff_capsules.iter().enumerate() {
        if let Some(mapped) = metadata_from_source_entry(
            source_key,
            issue,
            issue_id,
            "handoff_capsules",
            entry,
            idx,
            created_at,
            updated_at,
        )? {
            out.push(mapped);
        }
    }
    Ok(out)
}

fn metadata_from_legacy_text(
    source_key: &str,
    issue: &SourceIssue,
    issue_id: &str,
    kind: &str,
    text: &str,
    index: usize,
    fallback_datetime: &str,
) -> Option<MetadataEntry> {
    let content = normalize_non_empty(Some(text))?;
    let username = normalize_text(
        issue
            .owner
            .as_deref()
            .or(issue.created_by.as_deref())
            .or(Some("unknown")),
        "unknown",
    );
    let datetime = normalize_datetime(Some(fallback_datetime))
        .unwrap_or_else(|| fallback_datetime.to_string());
    Some(MetadataEntry {
        entry_id: deterministic_entry_id(
            source_key, issue_id, kind, index, &content, &username, &datetime, "unknown",
            "unknown", "unknown",
        ),
        content,
        username,
        datetime,
        agentname: "unknown".to_string(),
        model: "unknown".to_string(),
        version: "unknown".to_string(),
    })
}

#[allow(clippy::too_many_arguments)]
fn metadata_from_source_entry(
    source_key: &str,
    issue: &SourceIssue,
    issue_id: &str,
    kind: &str,
    entry: &SourceMetadataEntry,
    index: usize,
    created_at: &str,
    updated_at: &str,
) -> Result<Option<MetadataEntry>, ImportError> {
    let Some(content) = entry
        .content
        .as_deref()
        .and_then(|value| normalize_non_empty(Some(value)))
    else {
        return Ok(None);
    };
    let username = normalize_text(
        entry
            .username
            .as_deref()
            .or(issue.owner.as_deref())
            .or(issue.created_by.as_deref())
            .or(Some("unknown")),
        "unknown",
    );
    let datetime = normalize_datetime(
        entry
            .datetime
            .as_deref()
            .or(issue.updated_at.as_deref())
            .or(issue.created_at.as_deref())
            .or(Some(updated_at))
            .or(Some(created_at)),
    )
    .unwrap_or_else(|| updated_at.to_string());
    let agentname = normalize_text(entry.agentname.as_deref().or(Some("unknown")), "unknown");
    let model = normalize_text(entry.model.as_deref().or(Some("unknown")), "unknown");
    let version = normalize_text(entry.version.as_deref().or(Some("unknown")), "unknown");
    let entry_id = normalize_non_empty(entry.entry_id.as_deref()).unwrap_or_else(|| {
        deterministic_entry_id(
            source_key, issue_id, kind, index, &content, &username, &datetime, &agentname, &model,
            &version,
        )
    });
    Ok(Some(MetadataEntry {
        entry_id,
        content,
        username,
        datetime,
        agentname,
        model,
        version,
    }))
}

#[allow(clippy::too_many_arguments)]
fn deterministic_entry_id(
    source_key: &str,
    issue_id: &str,
    kind: &str,
    index: usize,
    content: &str,
    username: &str,
    datetime: &str,
    agentname: &str,
    model: &str,
    version: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source_key.as_bytes());
    hasher.update(b"|");
    hasher.update(issue_id.as_bytes());
    hasher.update(b"|");
    hasher.update(kind.as_bytes());
    hasher.update(b"|");
    hasher.update(index.to_string().as_bytes());
    hasher.update(b"|");
    hasher.update(content.as_bytes());
    hasher.update(b"|");
    hasher.update(username.as_bytes());
    hasher.update(b"|");
    hasher.update(datetime.as_bytes());
    hasher.update(b"|");
    hasher.update(agentname.as_bytes());
    hasher.update(b"|");
    hasher.update(model.as_bytes());
    hasher.update(b"|");
    hasher.update(version.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{:02x}", byte);
    }
    out
}

#[cfg(test)]
#[path = "service_tests_ext.rs"]
mod tests_ext;
