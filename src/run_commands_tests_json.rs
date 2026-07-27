//! JSON output regressions for `kno show -j`.

use super::*;

#[test]
fn show_json_is_rfc8259_valid_with_multiline_metadata() {
    // Multiline descriptions and handoff capsules must serialize as
    // RFC 8259-valid JSON: every control character escaped, so any
    // strict parser (python json.load, jq) accepts the output.
    let gnarly = "line one\nline two\r\n\ttabbed\u{1b}[31mansi";
    let entry = crate::domain::metadata::MetadataEntry {
        entry_id: "e1".to_string(),
        content: gnarly.to_string(),
        username: "user".to_string(),
        datetime: "2026-07-27T00:00:00Z".to_string(),
        agentname: "agent".to_string(),
        model: "model".to_string(),
        version: "1".to_string(),
    };
    let knot = app::KnotView {
        id: "K-multiline".to_string(),
        alias: None,
        title: "Multiline".to_string(),
        state: "planning".to_string(),
        updated_at: "2026-07-27T00:00:00Z".to_string(),
        body: Some(gnarly.to_string()),
        description: Some(gnarly.to_string()),
        acceptance: None,
        priority: None,
        knot_type: crate::domain::knot_type::KnotType::Work,
        tags: Vec::new(),
        notes: vec![entry.clone()],
        handoff_capsules: vec![entry.clone(), entry],
        invariants: Vec::new(),
        verification_steps: Vec::new(),
        step_history: Vec::new(),
        gate: None,
        lease: None,
        execution_plan: None,
        scope: None,
        lease_id: None,
        lease_expiry_ts: 0,
        lease_agent: None,
        workflow_id: "work_sdlc".to_string(),
        profile_id: "autopilot".to_string(),
        profile_etag: None,
        deferred_from_state: None,
        blocked_from_state: None,
        created_at: None,
        step_metadata: None,
        next_step_metadata: None,
        edges: Vec::new(),
        child_summaries: Vec::new(),
    };

    for verbose in [false, true] {
        let mut value = show_json_value(&knot);
        trim_show_json_metadata(&mut value, &knot, verbose);
        let rendered = serde_json::to_string_pretty(&value).expect("serialize");
        for ch in rendered.chars() {
            assert!(
                ch == '\n' || !ch.is_control(),
                "raw control character {ch:?} in show -j output"
            );
        }
        let parsed: serde_json::Value =
            serde_json::from_str(&rendered).expect("show -j output must re-parse as JSON");
        assert_eq!(
            parsed["handoff_capsules"]
                .as_array()
                .expect("capsules array")
                .last()
                .expect("capsule present")["content"]
                .as_str(),
            Some("line one\nline two\r\n\ttabbed\u{1b}[31mansi"),
            "capsule content must round-trip"
        );
    }
}
