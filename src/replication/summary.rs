//! Result types for the push / pull halves of replication.

use serde::Serialize;

use crate::sync::SyncSummary;

/// What the push half published (or found already published).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PushSummary {
    pub local_event_files: u64,
    pub copied_files: u64,
    pub committed: bool,
    pub pushed: bool,
    pub commit: Option<String>,
}

/// A full `push + pull` round trip.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReplicationSummary {
    pub push: PushSummary,
    pub pull: SyncSummary,
}

/// Result of a `kno sync`.
///
/// Neither half blocks wholesale on an active lease: push never has, and
/// pull now filters per knot instead of deferring entirely -- see
/// `SyncSummary::held_back_knots` for what a locally leased knot skips.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "status")]
pub enum SyncOutcome {
    #[serde(rename = "completed")]
    Completed(ReplicationSummary),
}

impl PushSummary {
    /// One-line human-readable rendering of the push half.
    pub fn render(&self) -> String {
        format!(
            "push(local_event_files={} copied_files={} committed={} pushed={})",
            self.local_event_files, self.copied_files, self.committed, self.pushed
        )
    }
}

impl SyncOutcome {
    /// One-line human-readable rendering for `kno sync` without `--json`.
    pub fn render(&self) -> String {
        match self {
            SyncOutcome::Completed(summary) => format!(
                "sync {} pull(head={} index_files={} full_files={} \
                 knot_updates={} edge_adds={} edge_removes={}{})",
                summary.push.render(),
                summary.pull.target_head,
                summary.pull.index_files,
                summary.pull.full_files,
                summary.pull.knot_updates,
                summary.pull.edge_adds,
                summary.pull.edge_removes,
                held_back_note(&summary.pull.held_back_knots),
            ),
        }
    }
}

fn held_back_note(held_back_knots: &[String]) -> String {
    if held_back_knots.is_empty() {
        String::new()
    } else {
        format!(
            " held_back=[{}] (locally leased)",
            held_back_knots.join(",")
        )
    }
}

#[cfg(test)]
#[path = "summary_tests.rs"]
mod tests;
