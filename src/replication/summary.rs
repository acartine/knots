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

/// Result of a `kno sync` that gracefully handles active leases.
///
/// Push is never blocked by a lease, so the deferred arm still carries the
/// push summary: only the pull half is postponed.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "status")]
pub enum SyncOutcome {
    #[serde(rename = "completed")]
    Completed(ReplicationSummary),
    #[serde(rename = "deferred")]
    Deferred {
        active_leases: i64,
        push: PushSummary,
    },
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
                 knot_updates={} edge_adds={} edge_removes={})",
                summary.push.render(),
                summary.pull.target_head,
                summary.pull.index_files,
                summary.pull.full_files,
                summary.pull.knot_updates,
                summary.pull.edge_adds,
                summary.pull.edge_removes
            ),
            SyncOutcome::Deferred {
                active_leases,
                push,
            } => format!(
                "sync {} pull deferred: {} active lease(s); \
                 pull will run when leases are terminated",
                push.render(),
                active_leases
            ),
        }
    }
}

#[cfg(test)]
#[path = "summary_tests.rs"]
mod tests;
