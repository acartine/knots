use std::io::{BufWriter, Write};

use crate::app::{AppError, KnotView};

/// Write knots as NDJSON (one JSON object per line) to stdout.
///
/// Each line is a compact, independently-parseable JSON object.
/// After all knot lines, emits a metadata line with `_meta: true`,
/// the total count, and `complete: true`.
///
/// Flushes after every line so pipe consumers see partial results.
pub fn stream_ndjson_knots(knots: &[KnotView]) -> Result<(), AppError> {
    let stdout = std::io::stdout();
    let mut writer = BufWriter::new(stdout.lock());
    for knot in knots {
        let line = serde_json::to_string(knot).map_err(|e| {
            AppError::InvalidArgument(format!("json serialize: {e}"))
        })?;
        writeln!(writer, "{line}").map_err(io_error)?;
        writer.flush().map_err(io_error)?;
    }
    let meta = serde_json::json!({
        "_meta": true,
        "total": knots.len(),
        "complete": true
    });
    writeln!(writer, "{meta}").map_err(io_error)?;
    writer.flush().map_err(io_error)?;
    Ok(())
}

fn io_error(e: std::io::Error) -> AppError {
    AppError::InvalidArgument(format!("stream write: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::knot_type::KnotType;

    fn sample_knot(id: &str, title: &str, state: &str) -> KnotView {
        KnotView {
            id: id.to_string(),
            alias: None,
            title: title.to_string(),
            state: state.to_string(),
            updated_at: "2026-04-06T00:00:00Z".to_string(),
            body: None,
            description: None,
            acceptance: None,
            priority: None,
            knot_type: KnotType::Work,
            tags: Vec::new(),
            notes: Vec::new(),
            handoff_capsules: Vec::new(),
            invariants: Vec::new(),
            step_history: Vec::new(),
            gate: None,
            lease: None,
            lease_id: None,
            lease_expiry_ts: 0,
            lease_agent: None,
            workflow_id: "compatibility".to_string(),
            profile_id: "default".to_string(),
            profile_etag: None,
            deferred_from_state: None,
            blocked_from_state: None,
            created_at: None,
            step_metadata: None,
            next_step_metadata: None,
            edges: Vec::new(),
            child_summaries: Vec::new(),
        }
    }

    /// Write NDJSON to a Vec<u8> buffer instead of stdout for testing.
    fn stream_to_buffer(knots: &[KnotView]) -> Result<Vec<u8>, AppError> {
        let mut buf = Vec::new();
        for knot in knots {
            let line = serde_json::to_string(knot).map_err(|e| {
                AppError::InvalidArgument(format!("json serialize: {e}"))
            })?;
            writeln!(buf, "{line}").map_err(io_error)?;
        }
        let meta = serde_json::json!({
            "_meta": true,
            "total": knots.len(),
            "complete": true
        });
        writeln!(buf, "{meta}").map_err(io_error)?;
        Ok(buf)
    }

    #[test]
    fn ndjson_each_line_is_valid_json() {
        let knots = vec![
            sample_knot("K-1", "First", "planning"),
            sample_knot("K-2", "Second", "implementing"),
        ];
        let buf = stream_to_buffer(&knots).expect("stream");
        let output = String::from_utf8(buf).expect("utf8");
        let lines: Vec<&str> = output.lines().collect();

        assert_eq!(lines.len(), 3, "2 knots + 1 metadata");
        for line in &lines {
            let parsed: serde_json::Value =
                serde_json::from_str(line).expect("each line valid JSON");
            assert!(parsed.is_object());
        }
    }

    #[test]
    fn ndjson_metadata_line_has_correct_total() {
        let knots = vec![
            sample_knot("K-1", "First", "planning"),
            sample_knot("K-2", "Second", "implementing"),
            sample_knot("K-3", "Third", "shipped"),
        ];
        let buf = stream_to_buffer(&knots).expect("stream");
        let output = String::from_utf8(buf).expect("utf8");
        let last_line = output.lines().last().expect("has lines");
        let meta: serde_json::Value =
            serde_json::from_str(last_line).expect("meta valid JSON");

        assert_eq!(meta["_meta"], serde_json::json!(true));
        assert_eq!(meta["total"], serde_json::json!(3));
        assert_eq!(meta["complete"], serde_json::json!(true));
    }

    #[test]
    fn ndjson_knot_lines_contain_expected_ids() {
        let knots = vec![
            sample_knot("K-1", "First", "planning"),
            sample_knot("K-2", "Second", "implementing"),
        ];
        let buf = stream_to_buffer(&knots).expect("stream");
        let output = String::from_utf8(buf).expect("utf8");
        let lines: Vec<&str> = output.lines().collect();

        let k1: serde_json::Value =
            serde_json::from_str(lines[0]).expect("line 0");
        assert_eq!(k1["id"], serde_json::json!("K-1"));

        let k2: serde_json::Value =
            serde_json::from_str(lines[1]).expect("line 1");
        assert_eq!(k2["id"], serde_json::json!("K-2"));
    }

    #[test]
    fn ndjson_empty_list_emits_only_metadata() {
        let knots: Vec<KnotView> = Vec::new();
        let buf = stream_to_buffer(&knots).expect("stream");
        let output = String::from_utf8(buf).expect("utf8");
        let lines: Vec<&str> = output.lines().collect();

        assert_eq!(lines.len(), 1, "only metadata line");
        let meta: serde_json::Value =
            serde_json::from_str(lines[0]).expect("meta valid JSON");
        assert_eq!(meta["_meta"], serde_json::json!(true));
        assert_eq!(meta["total"], serde_json::json!(0));
        assert_eq!(meta["complete"], serde_json::json!(true));
    }

    #[test]
    fn ndjson_lines_are_compact_no_array_wrapper() {
        let knots = vec![sample_knot("K-1", "First", "planning")];
        let buf = stream_to_buffer(&knots).expect("stream");
        let output = String::from_utf8(buf).expect("utf8");

        // No line should start with '[' (no array wrapper)
        for line in output.lines() {
            assert!(!line.starts_with('['), "must not use array wrapper");
        }
        // Each knot line should be a single line (no embedded newlines)
        let first_line = output.lines().next().expect("has line");
        assert!(!first_line.contains('\n'));
    }
}
