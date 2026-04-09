use std::collections::HashSet;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;

use rusqlite::Connection;
use serde::Serialize;

use crate::installed_workflows;
use crate::progress::{emit_progress, ProgressKind, ProgressReporter};
use crate::project::StorePaths;

mod apply;
mod git;
mod worktree;

use apply::IncrementalApplier;
pub use git::GitAdapter;
pub use worktree::KnotsWorktree;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SyncSummary {
    pub target_head: String,
    pub index_files: u64,
    pub full_files: u64,
    pub knot_updates: u64,
    pub edge_adds: u64,
    pub edge_removes: u64,
}

pub struct SyncService<'a> {
    conn: &'a Connection,
    repo_root: PathBuf,
    store_paths: StorePaths,
    git: GitAdapter,
}

impl<'a> SyncService<'a> {
    #[cfg(test)]
    pub fn new(conn: &'a Connection, repo_root: PathBuf) -> Self {
        let store_paths = StorePaths {
            root: repo_root.join(".knots"),
        };
        Self::with_store_paths(conn, repo_root, store_paths)
    }

    fn known_workflow_ids(&self) -> HashSet<String> {
        if let Ok(registry) = installed_workflows::InstalledWorkflowRegistry::load(&self.repo_root)
        {
            registry.list().iter().map(|w| w.id.clone()).collect()
        } else {
            crate::domain::knot_type::KnotType::ALL
                .into_iter()
                .map(installed_workflows::builtin_workflow_id_for_knot_type)
                .collect()
        }
    }

    pub fn with_store_paths(
        conn: &'a Connection,
        repo_root: PathBuf,
        store_paths: StorePaths,
    ) -> Self {
        Self {
            conn,
            repo_root,
            store_paths,
            git: GitAdapter::new(),
        }
    }

    #[allow(dead_code)]
    pub fn sync(&self) -> Result<SyncSummary, SyncError> {
        let mut reporter = None;
        self.sync_with_progress(&mut reporter)
    }

    pub fn sync_with_progress(
        &self,
        reporter: &mut Option<&mut dyn ProgressReporter>,
    ) -> Result<SyncSummary, SyncError> {
        emit_progress(reporter, ProgressKind::Stage, "importing knots updates")?;
        let worktree = KnotsWorktree::with_store_paths(self.repo_root.clone(), &self.store_paths);
        emit_progress(reporter, ProgressKind::Info, "preparing knots worktree")?;
        worktree.ensure_exists(&self.git)?;

        let target_head = match self.git.fetch_branch_with_filter(
            &self.repo_root,
            worktree.remote(),
            worktree.branch(),
            crate::db::get_sync_fetch_blob_limit_kb(self.conn)?,
        ) {
            Ok(()) => {
                let remote_ref = format!("{}/{}", worktree.remote(), worktree.branch());
                emit_progress(
                    reporter,
                    ProgressKind::Info,
                    format!("resetting knots worktree to {remote_ref}"),
                )?;
                let head = self.git.rev_parse(&self.repo_root, &remote_ref)?;
                self.git.reset_hard(worktree.path(), &head)?;
                head
            }
            Err(err) if err.is_missing_remote() => {
                emit_progress(
                    reporter,
                    ProgressKind::Warn,
                    "origin/knots is unavailable; using local knots worktree state",
                )?;
                self.git.rev_parse(worktree.path(), "HEAD")?
            }
            Err(err) => return Err(err),
        };

        worktree.ensure_clean(&self.git)?;
        emit_progress(
            reporter,
            ProgressKind::Info,
            "applying knots events to the local cache",
        )?;

        let known = self.known_workflow_ids();
        let mut applier = IncrementalApplier::new(
            self.conn,
            worktree.path().to_path_buf(),
            self.git.clone(),
            known,
        );
        let summary = applier.apply_to_head(&target_head)?;
        emit_progress(
            reporter,
            ProgressKind::Success,
            format!(
                "pull complete at {} (index={}, full={})",
                short_commit(&summary.target_head),
                summary.index_files,
                summary.full_files
            ),
        )?;
        Ok(summary)
    }
}

fn short_commit(commit: &str) -> &str {
    &commit[..commit.len().min(12)]
}

#[derive(Debug)]
pub enum SyncError {
    Io(std::io::Error),
    Db(rusqlite::Error),
    GitUnavailable,
    GitCommandFailed {
        command: String,
        code: Option<i32>,
        stderr: String,
    },
    DirtyWorktree(PathBuf),
    InvalidEvent {
        path: PathBuf,
        message: String,
    },
    FileConflict {
        path: PathBuf,
    },
    MergeConflictEscalation {
        message: String,
    },
    SnapshotLoad {
        message: String,
    },
    ActiveLeasesExist(i64),
}

impl SyncError {
    pub fn is_missing_remote(&self) -> bool {
        match self {
            SyncError::GitCommandFailed { stderr, .. } => {
                let lower = stderr.to_ascii_lowercase();
                lower.contains("no such remote")
                    || lower.contains("could not read from remote repository")
                    || lower.contains("does not appear to be a git repository")
            }
            _ => false,
        }
    }

    pub fn is_unknown_revision(&self) -> bool {
        match self {
            SyncError::GitCommandFailed { stderr, .. } => {
                stderr.contains("unknown revision")
                    || stderr.contains("bad object")
                    || stderr.contains("bad revision")
            }
            _ => false,
        }
    }

    #[allow(dead_code)]
    pub fn is_active_leases(&self) -> bool {
        matches!(self, SyncError::ActiveLeasesExist(_))
    }

    pub fn is_non_fast_forward(&self) -> bool {
        match self {
            SyncError::GitCommandFailed { stderr, .. } => {
                let lower = stderr.to_ascii_lowercase();
                lower.contains("non-fast-forward")
                    || lower.contains("fetch first")
                    || lower.contains("rejected")
            }
            _ => false,
        }
    }
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncError::Io(err) => write!(f, "I/O error: {}", err),
            SyncError::Db(err) => write!(f, "database error: {}", err),
            SyncError::GitUnavailable => write!(f, "git CLI is not installed"),
            SyncError::GitCommandFailed {
                command,
                code,
                stderr,
            } => {
                write!(
                    f,
                    "git command failed (code {:?}): {} ({})",
                    code, command, stderr
                )
            }
            SyncError::DirtyWorktree(path) => write!(
                f,
                "knots worktree '{}' has uncommitted changes",
                path.display()
            ),
            SyncError::InvalidEvent { path, message } => {
                write!(f, "invalid event '{}': {}", path.display(), message)
            }
            SyncError::FileConflict { path } => {
                write!(
                    f,
                    "push conflict on '{}': local event file collides with remote content",
                    path.display()
                )
            }
            SyncError::MergeConflictEscalation { message } => {
                write!(f, "merge conflict escalation: {}", message)
            }
            SyncError::SnapshotLoad { message } => {
                write!(f, "snapshot load failed: {}", message)
            }
            SyncError::ActiveLeasesExist(count) => {
                write!(
                    f,
                    "{} active lease(s) found; \
                     terminate leases before syncing",
                    count
                )
            }
        }
    }
}

impl Error for SyncError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            SyncError::Io(err) => Some(err),
            SyncError::Db(err) => Some(err),
            SyncError::GitUnavailable => None,
            SyncError::GitCommandFailed { .. } => None,
            SyncError::DirtyWorktree(_) => None,
            SyncError::InvalidEvent { .. } => None,
            SyncError::FileConflict { .. } => None,
            SyncError::MergeConflictEscalation { .. } => None,
            SyncError::SnapshotLoad { .. } => None,
            SyncError::ActiveLeasesExist(_) => None,
        }
    }
}

impl From<std::io::Error> for SyncError {
    fn from(value: std::io::Error) -> Self {
        SyncError::Io(value)
    }
}

impl From<rusqlite::Error> for SyncError {
    fn from(value: rusqlite::Error) -> Self {
        SyncError::Db(value)
    }
}

#[cfg(test)]
#[path = "error_tests.rs"]
mod error_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
#[path = "tests_ext.rs"]
mod tests_ext;
