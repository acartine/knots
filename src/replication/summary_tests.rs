use super::{PushSummary, ReplicationSummary, SyncOutcome};
use crate::sync::SyncSummary;

fn push(copied_files: u64, pushed: bool) -> PushSummary {
    PushSummary {
        local_event_files: 12,
        copied_files,
        committed: pushed,
        pushed,
        commit: pushed.then(|| "9f2c".to_string()),
    }
}

fn pull(held_back_knots: Vec<String>) -> SyncSummary {
    SyncSummary {
        target_head: "abc123".to_string(),
        index_files: 4,
        full_files: 5,
        knot_updates: 6,
        edge_adds: 7,
        edge_removes: 8,
        held_back_knots,
    }
}

#[test]
fn completed_render_reports_both_halves() {
    let outcome = SyncOutcome::Completed(ReplicationSummary {
        push: push(3, true),
        pull: pull(Vec::new()),
    });
    assert_eq!(
        outcome.render(),
        "sync push(local_event_files=12 copied_files=3 committed=true pushed=true) \
         pull(head=abc123 index_files=4 full_files=5 knot_updates=6 edge_adds=7 \
         edge_removes=8)"
    );
}

#[test]
fn completed_render_names_held_back_knots() {
    let outcome = SyncOutcome::Completed(ReplicationSummary {
        push: push(3, true),
        pull: pull(vec!["K-1".to_string(), "K-2".to_string()]),
    });
    assert_eq!(
        outcome.render(),
        "sync push(local_event_files=12 copied_files=3 committed=true pushed=true) \
         pull(head=abc123 index_files=4 full_files=5 knot_updates=6 edge_adds=7 \
         edge_removes=8 held_back=[K-1,K-2] (locally leased))"
    );
}

#[test]
fn completed_json_exposes_held_back_knots() {
    let outcome = SyncOutcome::Completed(ReplicationSummary {
        push: push(0, false),
        pull: pull(vec!["K-1".to_string()]),
    });
    let value = serde_json::to_value(&outcome).expect("outcome should serialize");
    assert_eq!(value["status"], "completed");
    assert_eq!(value["pull"]["held_back_knots"][0], "K-1");
}
