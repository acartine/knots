use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::progress::{emit_progress, ProgressKind, ProgressReporter};
use crate::project::StorePaths;
use crate::sync::{GitAdapter, KnotsWorktree, SyncError, SyncService, SyncSummary};

mod generation;
mod legacy_push;
mod outbox;
mod summary;

pub use summary::{PushSummary, ReplicationSummary, SyncOutcome};

enum PushAttemptResult {
    Success(PushSummary),
    AlreadySynced(PushSummary),
    Retry(SyncError),
}

pub struct ReplicationService<'a> {
    conn: &'a Connection,
    repo_root: PathBuf,
    store_paths: StorePaths,
    git: GitAdapter,
}

impl<'a> ReplicationService<'a> {
    #[cfg(test)]
    pub fn new(conn: &'a Connection, repo_root: PathBuf) -> Self {
        let store_paths = StorePaths {
            root: repo_root.join(".knots"),
        };
        Self::with_store_paths(conn, repo_root, store_paths)
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
    pub fn pull(&self) -> Result<SyncSummary, SyncError> {
        let service = SyncService::with_store_paths(
            self.conn,
            self.repo_root.clone(),
            self.store_paths.clone(),
        );
        service.sync()
    }

    pub fn pull_with_progress(
        &self,
        reporter: &mut Option<&mut dyn ProgressReporter>,
    ) -> Result<SyncSummary, SyncError> {
        let service = SyncService::with_store_paths(
            self.conn,
            self.repo_root.clone(),
            self.store_paths.clone(),
        );
        service.sync_with_progress(reporter)
    }

    pub fn push(&self) -> Result<PushSummary, SyncError> {
        let mut reporter = None;
        self.push_with_progress(&mut reporter)
    }

    pub fn push_with_progress(
        &self,
        reporter: &mut Option<&mut dyn ProgressReporter>,
    ) -> Result<PushSummary, SyncError> {
        // Deliberately unguarded by leases: event files are immutable and
        // uuidv7-named, so publishing mid-claim exports nothing the local
        // store does not already hold. Only pull can move a knot out from
        // under an active claim.
        const MAX_ATTEMPTS: usize = 3;

        emit_progress(
            reporter,
            ProgressKind::Stage,
            "publishing local knots events",
        )?;
        let worktree = KnotsWorktree::with_store_paths(self.repo_root.clone(), &self.store_paths);
        emit_progress(reporter, ProgressKind::Info, "preparing knots worktree")?;
        worktree.ensure_exists(&self.git)?;

        for attempt in 0..MAX_ATTEMPTS {
            match self.attempt_push(&worktree, reporter)? {
                PushAttemptResult::Success(summary) => return Ok(summary),
                PushAttemptResult::AlreadySynced(summary) => return Ok(summary),
                PushAttemptResult::Retry(err) if attempt + 1 < MAX_ATTEMPTS => {
                    emit_progress(
                        reporter,
                        ProgressKind::Warn,
                        format!(
                            "push was rejected; refreshing remote \
                             state and retrying ({}/{})",
                            attempt + 2,
                            MAX_ATTEMPTS
                        ),
                    )?;
                    let _ = err;
                    continue;
                }
                PushAttemptResult::Retry(_) => {
                    return Err(SyncError::MergeConflictEscalation {
                        message: "push rejected as non-fast-forward \
                                  after retries"
                            .to_string(),
                    });
                }
            }
        }

        Err(SyncError::MergeConflictEscalation {
            message: "push retries exhausted".to_string(),
        })
    }

    fn attempt_push(
        &self,
        worktree: &KnotsWorktree,
        reporter: &mut Option<&mut dyn ProgressReporter>,
    ) -> Result<PushAttemptResult, SyncError> {
        let selection = self.prepare_push_files(worktree, reporter)?;
        let local_event_files = selection.physical_count;
        if selection.files.is_empty() {
            return self.already_synced(
                &selection,
                reporter,
                0,
                "no eligible local knots events found; nothing to push",
            );
        }
        emit_progress(
            reporter,
            ProgressKind::Info,
            format!(
                "checking {local_event_files} local knot file(s) \
                 against the publish worktree"
            ),
        )?;
        let copied_paths = self.copy_files_into_worktree(worktree.path(), &selection.files)?;
        let copied_files = copied_paths.len() as u64;
        if copied_files > 0 {
            emit_progress(
                reporter,
                ProgressKind::Info,
                format!("copied {copied_files} local knot file(s) into the publish worktree"),
            )?;
        }
        if copied_paths.is_empty() {
            return self.already_synced(
                &selection,
                reporter,
                copied_files,
                "remote knots already includes the local events",
            );
        }

        self.git.add_path_bufs(worktree.path(), &copied_paths)?;

        if !self
            .git
            .has_staged_path_bufs(worktree.path(), &copied_paths)?
        {
            return self.already_synced(
                &selection,
                reporter,
                copied_files,
                "remote knots already includes the local events",
            );
        }

        emit_progress(reporter, ProgressKind::Info, "creating a publish commit")?;
        let commit = self
            .git
            .commit(worktree.path(), "knots: publish local events")?;

        emit_progress(
            reporter,
            ProgressKind::Info,
            format!("pushing knots data to {}", worktree.remote_display()),
        )?;
        match self
            .git
            .push_refspec(worktree.path(), worktree.remote(), &worktree.push_refspec())
        {
            Ok(()) => {
                self.save_push_baseline(&selection)?;
                emit_progress(
                    reporter,
                    ProgressKind::Success,
                    format!("push complete at {}", short_commit(&commit)),
                )?;
                Ok(PushAttemptResult::Success(PushSummary {
                    local_event_files,
                    copied_files,
                    committed: true,
                    pushed: true,
                    commit: Some(commit),
                }))
            }
            Err(err) if err.is_non_fast_forward() => Ok(PushAttemptResult::Retry(err)),
            Err(err) if err.is_ref_policy_rejection() => Err(err),
            Err(err) => Err(err),
        }
    }

    fn already_synced(
        &self,
        selection: &legacy_push::Selection,
        reporter: &mut Option<&mut dyn ProgressReporter>,
        copied_files: u64,
        message: &str,
    ) -> Result<PushAttemptResult, SyncError> {
        self.save_push_baseline(selection)?;
        emit_progress(reporter, ProgressKind::Success, message)?;
        Ok(PushAttemptResult::AlreadySynced(PushSummary {
            local_event_files: selection.physical_count,
            copied_files,
            committed: false,
            pushed: false,
            commit: None,
        }))
    }

    fn save_push_baseline(&self, selection: &legacy_push::Selection) -> Result<(), SyncError> {
        if let Some(baseline) = &selection.baseline {
            legacy_push::save_baseline(self.conn, baseline)?;
        }
        Ok(())
    }

    fn prepare_push_files(
        &self,
        worktree: &KnotsWorktree,
        reporter: &mut Option<&mut dyn ProgressReporter>,
    ) -> Result<legacy_push::Selection, SyncError> {
        let active_head = self.reset_worktree_to_remote_or_local(worktree, reporter)?;
        worktree.ensure_clean(&self.git)?;
        if worktree.is_generation_two() {
            emit_progress(
                reporter,
                ProgressKind::Info,
                "validating the active compaction generation",
            )?;
            generation::fence_local_store(
                &self.git,
                &self.repo_root,
                &self.store_paths,
                worktree.path(),
                &active_head,
            )?;
        }
        emit_progress(
            reporter,
            ProgressKind::Info,
            "scanning local knots event files",
        )?;
        legacy_push::select(self.conn, &self.store_paths)
    }

    pub fn sync(&self) -> Result<ReplicationSummary, SyncError> {
        let mut reporter = None;
        self.sync_with_progress(&mut reporter)
    }

    pub fn sync_with_progress(
        &self,
        reporter: &mut Option<&mut dyn ProgressReporter>,
    ) -> Result<ReplicationSummary, SyncError> {
        let push = self.push_with_progress(reporter)?;
        let pull = self.pull_with_progress(reporter)?;
        Ok(ReplicationSummary { push, pull })
    }

    /// Test-only composition of both halves. Production goes through `App`,
    /// which runs the push half under the repo lock alone and takes the cache
    /// lock only for `finish_sync_or_defer_with_progress`.
    #[cfg(test)]
    pub fn sync_or_defer_with_progress(
        &self,
        reporter: &mut Option<&mut dyn ProgressReporter>,
    ) -> Result<SyncOutcome, SyncError> {
        let push = self.push_with_progress(reporter)?;
        self.finish_sync_or_defer_with_progress(reporter, push)
    }

    /// Complete a sync whose push half already ran. Split out so callers can
    /// publish under the repo lock alone and hold the cache lock only for the
    /// pull half, which is the part that writes SQLite.
    ///
    /// The pull half no longer defers wholesale on an active local lease: it
    /// applies every knot except the ones this machine is mid-action on, and
    /// reports those in `pull.held_back_knots` (see `SyncSummary`).
    pub fn finish_sync_or_defer_with_progress(
        &self,
        reporter: &mut Option<&mut dyn ProgressReporter>,
        push: PushSummary,
    ) -> Result<SyncOutcome, SyncError> {
        let pull = self.pull_with_progress(reporter)?;
        Ok(SyncOutcome::Completed(ReplicationSummary { push, pull }))
    }

    pub fn count_unpushed_event_files(&self) -> Result<u64, SyncError> {
        let worktree = KnotsWorktree::with_store_paths(self.repo_root.clone(), &self.store_paths);
        worktree.ensure_exists(&self.git)?;
        let mut reporter = None;
        self.reset_worktree_to_remote_or_local(&worktree, &mut reporter)?;
        worktree.ensure_clean(&self.git)?;

        let local_files = legacy_push::select(self.conn, &self.store_paths)?.files;
        let mut unpushed = 0u64;
        for relative in local_files {
            if self.event_file_missing_or_changed(worktree.path(), &relative)? {
                unpushed += 1;
            }
        }
        Ok(unpushed)
    }

    fn reset_worktree_to_remote_or_local(
        &self,
        worktree: &KnotsWorktree,
        reporter: &mut Option<&mut dyn ProgressReporter>,
    ) -> Result<String, SyncError> {
        emit_progress(
            reporter,
            ProgressKind::Info,
            format!(
                "refreshing knots worktree from {}",
                worktree.remote_display()
            ),
        )?;
        match self.git.fetch_refspec_with_filter(
            &self.repo_root,
            worktree.remote(),
            &worktree.fetch_refspec(),
            crate::db::get_sync_fetch_blob_limit_kb(self.conn)?,
        ) {
            Ok(()) => {
                emit_progress(
                    reporter,
                    ProgressKind::Info,
                    format!("resetting knots worktree to {}", worktree.tracking_rev()),
                )?;
                let head = self
                    .git
                    .rev_parse(&self.repo_root, &worktree.tracking_rev())?;
                self.git.reset_hard(worktree.path(), &head)?;
                Ok(head)
            }
            Err(err) if err.is_missing_remote() || err.is_unknown_revision() => {
                emit_progress(
                    reporter,
                    ProgressKind::Warn,
                    format!(
                        "{} is unavailable; using local knots worktree state",
                        worktree.remote_display()
                    ),
                )?;
                let head = self.git.rev_parse(worktree.path(), "HEAD")?;
                self.git.reset_hard(worktree.path(), &head)?;
                Ok(head)
            }
            Err(err) => Err(err),
        }
    }

    fn copy_files_into_worktree(
        &self,
        worktree_root: &Path,
        relative_files: &[PathBuf],
    ) -> Result<Vec<PathBuf>, SyncError> {
        let mut copied = Vec::new();
        for relative in relative_files {
            let src = self.local_store_file_path(relative)?;
            if !src.exists() {
                continue;
            }
            let dst = worktree_root.join(relative);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }

            let src_bytes = std::fs::read(&src)?;
            if dst.exists() {
                let dst_bytes = std::fs::read(&dst)?;
                if dst_bytes == src_bytes {
                    continue;
                }
                return Err(SyncError::FileConflict {
                    path: relative.clone(),
                });
            }

            std::fs::write(&dst, src_bytes)?;
            copied.push(relative.clone());
        }

        Ok(copied)
    }

    fn event_file_missing_or_changed(
        &self,
        worktree_root: &Path,
        relative_file: &Path,
    ) -> Result<bool, SyncError> {
        let src = self.local_store_file_path(relative_file)?;
        if !src.exists() {
            return Ok(false);
        }

        let dst = worktree_root.join(relative_file);
        let src_bytes = std::fs::read(&src)?;
        if !dst.exists() {
            return Ok(true);
        }
        let dst_bytes = std::fs::read(&dst)?;
        Ok(dst_bytes != src_bytes)
    }

    fn local_store_file_path(&self, relative_file: &Path) -> Result<PathBuf, SyncError> {
        let store_relative =
            relative_file
                .strip_prefix(".knots")
                .map_err(|err| SyncError::InvalidEvent {
                    path: relative_file.to_path_buf(),
                    message: format!("expected .knots-relative event path: {}", err),
                })?;
        Ok(self.store_paths.root.join(store_relative))
    }
}

fn short_commit(commit: &str) -> &str {
    &commit[..commit.len().min(12)]
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "replication/tests_ref_policy.rs"]
mod tests_ref_policy;
