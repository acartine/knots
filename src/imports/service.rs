use std::fs::File;
use std::io::{BufRead, BufReader};

use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::db::{self, UpsertKnotHot};
use crate::events::{
    new_event_id, now_utc_rfc3339, EventRecord, EventWriter, FullEvent, FullEventKind, IndexEvent,
    IndexEventKind,
};

use super::errors::ImportError;
use super::source::{
    ensure_dolt_available, fetch_dolt_rows, map_dependency_kind, map_source_state, merged_body,
    normalize_path, parse_since, parse_timestamp, source_issue_from_dolt_row, source_key,
    SourceIssue, SourceKind,
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

    pub fn import_dolt(
        &self,
        repo: &str,
        since: Option<&str>,
        dry_run: bool,
    ) -> Result<ImportSummary, ImportError> {
        let source_ref = normalize_path(repo)?;
        ensure_dolt_available()?;
        let source_key = source_key(SourceKind::Dolt, &source_ref);
        let since_ts = parse_since(since)?;
        let previous_checkpoint = load_checkpoint(self.conn, &source_key)?;
        let mut run = ImportRun::new();

        let rows = fetch_dolt_rows(&source_ref)?;
        for (index, value) in rows.into_iter().enumerate() {
            let row_number = index + 1;
            run.checkpoint = Some(row_number.to_string());
            if let Some(cp) = previous_checkpoint {
                if row_number <= cp {
                    continue;
                }
            }

            run.processed_count += 1;
            let issue = match source_issue_from_dolt_row(value) {
                Ok(issue) => issue,
                Err(err) => {
                    run.error_count += 1;
                    run.last_error = Some(format!("row {}: {}", row_number, err));
                    continue;
                }
            };

            match self.import_issue(&source_ref, &source_key, issue, since_ts, dry_run) {
                Ok(Outcome::Imported) => run.imported_count += 1,
                Ok(Outcome::Skipped) => run.skipped_count += 1,
                Err(ImportError::InvalidRecord(message)) => {
                    run.error_count += 1;
                    run.last_error = Some(format!("row {}: {}", row_number, message));
                }
                Err(err) => {
                    run.last_error = Some(format!("row {}: {}", row_number, err));
                    return self.finish_run(
                        SourceKind::Dolt,
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
            SourceKind::Dolt,
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
        let token = fingerprint(source_key, &issue.id, &updated_at, action);
        if fingerprint_exists(self.conn, &token)? {
            return Ok(Outcome::Skipped);
        }

        if dry_run {
            return Ok(Outcome::Imported);
        }

        let state = map_source_state(&issue)?;
        let body = merged_body(&issue);
        let source_tag = format!("source:{}", source_ref);

        let created_event = FullEvent::with_identity(
            new_event_id(),
            created_at.clone(),
            issue.id.clone(),
            FullEventKind::KnotCreated.as_str(),
            json!({
                "title": issue.title,
                "state": state.as_str(),
                "body": body,
                "source": source_tag,
            }),
        );
        self.writer.write(&EventRecord::full(created_event))?;

        for label in &issue.labels {
            let event = FullEvent::with_identity(
                new_event_id(),
                created_at.clone(),
                issue.id.clone(),
                FullEventKind::KnotTagAdd.as_str(),
                json!({ "tag": label }),
            );
            self.writer.write(&EventRecord::full(event))?;
        }

        for dependency in &issue.dependencies {
            if let Some(depends_on) = dependency.depends_on_id.as_deref() {
                let kind = map_dependency_kind(dependency.dep_type.as_deref());
                let edge_event = FullEvent::with_identity(
                    new_event_id(),
                    created_at.clone(),
                    issue.id.clone(),
                    FullEventKind::KnotEdgeAdd.as_str(),
                    json!({"kind": kind, "dst": depends_on}),
                );
                self.writer.write(&EventRecord::full(edge_event))?;
                self.conn.execute(
                    "INSERT OR IGNORE INTO edge (src, kind, dst) VALUES (?1, ?2, ?3)",
                    params![issue.id, kind, depends_on],
                )?;
            }
        }

        if let Some(reason) = issue.close_reason.as_deref() {
            let comment_event = FullEvent::with_identity(
                new_event_id(),
                updated_at.clone(),
                issue.id.clone(),
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
                "knot_id": issue.id,
                "title": issue.title,
                "state": state.as_str(),
                "updated_at": updated_at,
                "terminal": state.is_terminal(),
            }),
        );
        self.writer.write(&EventRecord::index(index_event))?;

        db::upsert_knot_hot(
            self.conn,
            &UpsertKnotHot {
                id: &issue.id,
                title: &issue.title,
                state: state.as_str(),
                updated_at: &updated_at,
                body: body.as_deref(),
                workflow_etag: Some(&index_event_id),
                created_at: Some(&created_at),
            },
        )?;

        insert_fingerprint(
            self.conn,
            &token,
            source_key,
            &issue.id,
            &updated_at,
            action,
        )?;
        Ok(Outcome::Imported)
    }

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
