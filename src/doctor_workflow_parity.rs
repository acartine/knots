//! `workflow_id_parity` doctor check + --fix helper.
//!
//! Legacy `idx.knot_head` events predate the `workflow_id` field. Sync
//! infers `workflow_id` at apply time to populate the local cache, but the
//! shared event log stays "dirty" — other consumers must re-infer on
//! bootstrap. The check identifies knots whose latest `idx.knot_head` event
//! in the synced worktree lacks `workflow_id`; the fix appends a minimal
//! repair event carrying the resolved `workflow_id`, which publishes on the
//! next sync.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use rusqlite::Connection;
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::db;
use crate::doctor::{DoctorCheck, DoctorError, DoctorStatus};
use crate::events::{EventRecord, EventWriter, IndexEvent, IndexEventKind};
use crate::installed_workflows;
use crate::project::StorePaths;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct WorkflowParityFixSummary {
    pub emitted: usize,
    pub pending: usize,
    pub skipped: usize,
    pub failed: usize,
    pub messages: Vec<String>,
}

impl WorkflowParityFixSummary {
    pub(crate) fn needs_sync(&self) -> bool {
        self.emitted > 0 || self.pending > 0
    }

    pub(crate) fn first_message(&self) -> Option<&str> {
        self.messages.first().map(String::as_str)
    }

    fn note_pending(&mut self, head: &StaleHead) {
        self.pending += 1;
        self.messages
            .push(head.message("a newer local repair event is already waiting to sync"));
    }

    fn note_skipped(&mut self, head: &StaleHead, reason: &str) {
        self.skipped += 1;
        self.messages.push(head.message(reason));
    }

    fn note_failed(&mut self, head: &StaleHead, reason: &str) {
        self.failed += 1;
        self.messages.push(head.message(reason));
    }
}

enum RepairPayload {
    Ready(Value),
    Missing,
    Blocked(&'static str),
}

enum RepairEventResult {
    Emitted,
    Skipped(&'static str),
}

#[derive(Clone)]
struct StaleHead {
    knot_id: String,
    event_id: String,
    occurred_at: OffsetDateTime,
    occurred_at_raw: String,
    event_path: String,
    title: Option<String>,
    state: Option<String>,
    updated_at: Option<String>,
    profile_id: Option<String>,
    knot_type_str: Option<String>,
    terminal: Option<bool>,
    missing_fields: Vec<String>,
}

impl StaleHead {
    fn message(&self, reason: &str) -> String {
        format!(
            "{} event {} at {} ({}) {reason}",
            self.knot_id, self.event_id, self.occurred_at_raw, self.event_path
        )
    }
}

#[derive(Clone)]
struct ScanEntry {
    knot_id: String,
    event_id: String,
    occurred_at: OffsetDateTime,
    occurred_at_raw: String,
    event_path: String,
    has_workflow_id: bool,
    title: Option<String>,
    state: Option<String>,
    updated_at: Option<String>,
    profile_id: Option<String>,
    knot_type_str: Option<String>,
    terminal: Option<bool>,
    missing_fields: Vec<String>,
}

pub fn check_workflow_id_parity_at(store_paths: &StorePaths) -> Result<DoctorCheck, DoctorError> {
    let worktree_knots = store_paths.worktree_path().join(".knots");
    let stale = scan_stale_heads(&worktree_knots);
    if stale.is_empty() {
        return Ok(DoctorCheck::simple(
            "workflow_id_parity",
            DoctorStatus::Pass,
            "all latest idx.knot_head events include workflow_id",
        ));
    }
    Ok(DoctorCheck {
        name: "workflow_id_parity".to_string(),
        status: DoctorStatus::Warn,
        detail: stale_detail(&stale),
        data: Some(stale_heads_data(&stale)),
    })
}

/// Implements the `--fix` action for `workflow_id_parity`. For each knot with
/// a stale latest event, emits a minimal `idx.knot_head` event into
/// `.knots/index/...` using the DB's current state (hot), cold-catalog row, or
/// legacy stale-head payload. Failures are summarized so one bad row doesn't
/// abort the sweep.
pub(crate) fn fix_workflow_id_parity(repo_root: &Path) -> WorkflowParityFixSummary {
    let mut summary = WorkflowParityFixSummary::default();
    let store_paths = StorePaths {
        root: repo_root.join(".knots"),
    };
    let stale = scan_stale_heads(&store_paths.worktree_path().join(".knots"));
    if stale.is_empty() {
        return summary;
    }
    let db_path = store_paths.db_path();
    let Some(db_str) = db_path.to_str() else {
        return summary;
    };
    let Ok(conn) = db::open_connection(db_str) else {
        return summary;
    };
    let writer = EventWriter::with_db_path(store_paths.root.clone(), db_path);
    let local_latest = scan_latest_heads(&store_paths.root);
    for head in stale {
        if local_has_pending_repair(&local_latest, &head) {
            summary.note_pending(&head);
            continue;
        }
        match emit_repair_event(&conn, &writer, &head) {
            Ok(RepairEventResult::Emitted) => summary.emitted += 1,
            Ok(RepairEventResult::Skipped(reason)) => summary.note_skipped(&head, reason),
            Err(err) => summary.note_failed(&head, &err.to_string()),
        }
    }
    summary
}

fn emit_repair_event(
    conn: &Connection,
    writer: &EventWriter,
    head: &StaleHead,
) -> Result<RepairEventResult, Box<dyn std::error::Error>> {
    match build_repair_payload_from_hot(conn, head)? {
        RepairPayload::Ready(payload) => {
            write_repair_event(writer, payload)?;
            return Ok(RepairEventResult::Emitted);
        }
        RepairPayload::Blocked(reason) => return Ok(RepairEventResult::Skipped(reason)),
        RepairPayload::Missing => {}
    }
    if let Some(payload) = build_repair_payload_from_cold(conn, head)? {
        write_repair_event(writer, payload)?;
        return Ok(RepairEventResult::Emitted);
    }
    if let Some(payload) = build_repair_payload_from_stale(head) {
        write_repair_event(writer, payload)?;
        return Ok(RepairEventResult::Emitted);
    }
    Ok(RepairEventResult::Skipped(
        "stale head is absent from cache and lacks title, state, or updated_at",
    ))
}

fn write_repair_event(
    writer: &EventWriter,
    payload: Value,
) -> Result<(), Box<dyn std::error::Error>> {
    writer.write(&EventRecord::index(IndexEvent::new(
        IndexEventKind::KnotHead,
        payload,
    )))?;
    Ok(())
}

fn build_repair_payload_from_hot(
    conn: &Connection,
    head: &StaleHead,
) -> Result<RepairPayload, Box<dyn std::error::Error>> {
    let Some(record) = db::get_knot_hot(conn, &head.knot_id)? else {
        return Ok(RepairPayload::Missing);
    };
    if record.workflow_id.trim().is_empty() {
        return Ok(RepairPayload::Blocked(
            "hot cache row has an empty workflow_id",
        ));
    }
    let knot_type_str = record
        .knot_type
        .as_deref()
        .or(head.knot_type_str.as_deref())
        .unwrap_or("work");
    Ok(RepairPayload::Ready(json!({
        "knot_id": record.id,
        "title": record.title,
        "state": record.state,
        "updated_at": record.updated_at,
        "profile_id": record.profile_id,
        "workflow_id": record.workflow_id,
        "type": knot_type_str,
        "terminal": false,
    })))
}

fn build_repair_payload_from_cold(
    conn: &Connection,
    head: &StaleHead,
) -> Result<Option<Value>, Box<dyn std::error::Error>> {
    let Some(record) = db::get_cold_catalog(conn, &head.knot_id)? else {
        return Ok(None);
    };
    let knot_type_str = head.knot_type_str.as_deref().unwrap_or("work");
    let knot_type = crate::domain::knot_type::parse_knot_type(Some(knot_type_str));
    let workflow_id = installed_workflows::builtin_workflow_id_for_knot_type(knot_type);
    let profile_id = head.profile_id.as_deref().unwrap_or("autopilot");
    Ok(Some(json!({
        "knot_id": record.id,
        "title": record.title,
        "state": record.state,
        "updated_at": record.updated_at,
        "profile_id": profile_id,
        "workflow_id": workflow_id,
        "type": knot_type_str,
        "terminal": true,
    })))
}

fn build_repair_payload_from_stale(head: &StaleHead) -> Option<Value> {
    let title = head.title.as_deref()?;
    let state = head.state.as_deref()?;
    let updated_at = head.updated_at.as_deref()?;
    let knot_type = crate::domain::knot_type::parse_knot_type(head.knot_type_str.as_deref());
    let knot_type_str = head.knot_type_str.as_deref().unwrap_or(knot_type.as_str());
    let workflow_id = installed_workflows::builtin_workflow_id_for_knot_type(knot_type);
    let profile_id = head.profile_id.as_deref().unwrap_or("autopilot");
    let terminal = head.terminal.unwrap_or_else(|| inferred_terminal(state));
    Some(json!({
        "knot_id": &head.knot_id,
        "title": title,
        "state": state,
        "updated_at": updated_at,
        "profile_id": profile_id,
        "workflow_id": workflow_id,
        "type": knot_type_str,
        "terminal": terminal,
    }))
}

fn inferred_terminal(state: &str) -> bool {
    crate::tiering::TERMINAL_STATES
        .iter()
        .any(|terminal| state.eq_ignore_ascii_case(terminal))
}

fn scan_stale_heads(worktree_knots_root: &Path) -> Vec<StaleHead> {
    let latest = scan_latest_heads(worktree_knots_root);
    let mut stale: Vec<StaleHead> = latest
        .into_values()
        .filter_map(|entry| {
            if entry.has_workflow_id {
                None
            } else {
                Some(StaleHead {
                    knot_id: entry.knot_id,
                    event_id: entry.event_id,
                    occurred_at: entry.occurred_at,
                    occurred_at_raw: entry.occurred_at_raw,
                    event_path: entry.event_path,
                    title: entry.title,
                    state: entry.state,
                    updated_at: entry.updated_at,
                    profile_id: entry.profile_id,
                    knot_type_str: entry.knot_type_str,
                    terminal: entry.terminal,
                    missing_fields: entry.missing_fields,
                })
            }
        })
        .collect();
    stale.sort_by(|a, b| a.knot_id.cmp(&b.knot_id));
    stale
}

fn scan_latest_heads(worktree_knots_root: &Path) -> HashMap<String, ScanEntry> {
    let index_root = worktree_knots_root.join("index");
    if !index_root.exists() {
        return HashMap::new();
    }
    let mut latest: HashMap<String, ScanEntry> = HashMap::new();
    let mut stack = vec![index_root];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if is_knot_head_file(&path) {
                process_event_file(worktree_knots_root, &path, &mut latest);
            }
        }
    }
    latest
}

fn is_knot_head_file(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "json")
        && path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with("-idx.knot_head.json"))
}

fn process_event_file(
    worktree_knots_root: &Path,
    path: &Path,
    latest: &mut HashMap<String, ScanEntry>,
) {
    let Ok(bytes) = fs::read(path) else {
        return;
    };
    let Ok(event) = serde_json::from_slice::<Value>(&bytes) else {
        return;
    };
    let Some(data) = event.get("data").and_then(Value::as_object) else {
        return;
    };
    let Some(knot_id) = data.get("knot_id").and_then(Value::as_str) else {
        return;
    };
    let Some(occurred) = event.get("occurred_at").and_then(Value::as_str) else {
        return;
    };
    let Ok(ts) = OffsetDateTime::parse(occurred, &Rfc3339) else {
        return;
    };
    let has_workflow_id = data
        .get("workflow_id")
        .and_then(Value::as_str)
        .is_some_and(|v| !v.trim().is_empty());
    let knot_type_str = optional_data_string(data.get("type"));
    let mut missing_fields = Vec::new();
    if !has_workflow_id {
        missing_fields.push("workflow_id".to_string());
    }
    if knot_type_str.is_none() {
        missing_fields.push("type".to_string());
    }
    let new_entry = ScanEntry {
        knot_id: knot_id.to_string(),
        event_id: event_id_for(path, &event),
        occurred_at: ts,
        occurred_at_raw: occurred.to_string(),
        event_path: relative_event_path(worktree_knots_root, path),
        has_workflow_id,
        title: optional_data_string(data.get("title")),
        state: optional_data_string(data.get("state")),
        updated_at: optional_data_string(data.get("updated_at")),
        profile_id: optional_data_string(data.get("profile_id")),
        knot_type_str,
        terminal: data.get("terminal").and_then(Value::as_bool),
        missing_fields,
    };
    latest
        .entry(knot_id.to_string())
        .and_modify(|entry| {
            if ts > entry.occurred_at {
                *entry = new_entry.clone();
            }
        })
        .or_insert(new_entry);
}

fn local_has_pending_repair(latest: &HashMap<String, ScanEntry>, head: &StaleHead) -> bool {
    latest
        .get(&head.knot_id)
        .is_some_and(|entry| entry.has_workflow_id && entry.occurred_at > head.occurred_at)
}

fn stale_detail(stale: &[StaleHead]) -> String {
    let count = stale.len();
    let first = &stale[0];
    format!(
        "{count} knot(s) have a latest idx.knot_head event missing workflow_id; \
         first: {} event {} at {}; run doctor --fix to publish repair events",
        first.knot_id, first.event_id, first.event_path
    )
}

fn stale_heads_data(stale: &[StaleHead]) -> Value {
    let stale_heads: Vec<Value> = stale.iter().map(stale_head_data).collect();
    json!({ "stale_heads": stale_heads })
}

fn stale_head_data(head: &StaleHead) -> Value {
    json!({
        "knot_id": &head.knot_id,
        "event_id": &head.event_id,
        "occurred_at": &head.occurred_at_raw,
        "path": &head.event_path,
        "title": head.title.as_deref(),
        "state": head.state.as_deref(),
        "updated_at": head.updated_at.as_deref(),
        "profile_id": head.profile_id.as_deref(),
        "type": head.knot_type_str.as_deref(),
        "terminal": head.terminal,
        "missing_fields": &head.missing_fields,
    })
}

fn optional_data_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .map(str::to_string)
}

fn event_id_for(path: &Path, event: &Value) -> String {
    event
        .get("event_id")
        .and_then(Value::as_str)
        .filter(|raw| !raw.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            path.file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_suffix("-idx.knot_head.json"))
                .unwrap_or("unknown")
                .to_string()
        })
}

fn relative_event_path(worktree_knots_root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(worktree_knots_root).unwrap_or(path);
    Path::new(".knots").join(relative).display().to_string()
}

#[cfg(test)]
#[path = "doctor_workflow_parity_tests.rs"]
mod tests;
