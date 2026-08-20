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

#[test]
fn completed_render_reports_both_halves() {
    let outcome = SyncOutcome::Completed(ReplicationSummary {
        push: push(3, true),
        pull: SyncSummary {
            target_head: "abc123".to_string(),
            index_files: 4,
            full_files: 5,
            knot_updates: 6,
            edge_adds: 7,
            edge_removes: 8,
        },
    });
    assert_eq!(
        outcome.render(),
        "sync push(local_event_files=12 copied_files=3 committed=true pushed=true) \
         pull(head=abc123 index_files=4 full_files=5 knot_updates=6 edge_adds=7 \
         edge_removes=8)"
    );
}

#[test]
fn deferred_render_still_reports_what_the_push_published() {
    let outcome = SyncOutcome::Deferred {
        active_leases: 2,
        push: push(3, true),
    };
    assert_eq!(
        outcome.render(),
        "sync push(local_event_files=12 copied_files=3 committed=true pushed=true) \
         pull deferred: 2 active lease(s); pull will run when leases are terminated"
    );
}

#[test]
fn deferred_json_exposes_the_push_summary() {
    let outcome = SyncOutcome::Deferred {
        active_leases: 1,
        push: push(0, false),
    };
    let value = serde_json::to_value(&outcome).expect("outcome should serialize");
    assert_eq!(value["status"], "deferred");
    assert_eq!(value["active_leases"], 1);
    assert_eq!(value["push"]["copied_files"], 0);
    assert_eq!(value["push"]["pushed"], false);
    assert!(value["push"]["commit"].is_null());
}
