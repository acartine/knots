use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::Serialize;

use crate::progress::{emit_progress, ProgressKind, ProgressReporter};
use crate::sync::{GitAdapter, KnotsWorktree, SyncError, SyncService, SyncSummary};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PushSummary {
    pub local_event_files: u64,
    pub copied_files: u64,
    pub committed: bool,
    pub pushed: bool,
    pub commit: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReplicationSummary {
    pub push: PushSummary,
    pub pull: SyncSummary,
}

pub struct ReplicationService<'a> {
    conn: &'a Connection,
    repo_root: PathBuf,
    git: GitAdapter,
}

impl<'a> ReplicationService<'a> {
    pub fn new(conn: &'a Connection, repo_root: PathBuf) -> Self {
        Self {
            conn,
            repo_root,
            git: GitAdapter::new(),
        }
    }

    pub fn pull(&self) -> Result<SyncSummary, SyncError> {
        self.require_no_active_leases()?;
        let service = SyncService::new(self.conn, self.repo_root.clone());
        service.sync()
    }

    pub fn pull_with_progress(
        &self,
        reporter: &mut Option<&mut dyn ProgressReporter>,
    ) -> Result<SyncSummary, SyncError> {
        self.require_no_active_leases()?;
        let service = SyncService::new(self.conn, self.repo_root.clone());
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
        self.require_no_active_leases()?;
        const MAX_ATTEMPTS: usize = 3;

        emit_progress(
            reporter,
            ProgressKind::Stage,
            "publishing local knots events",
        )?;
        let worktree = KnotsWorktree::new(self.repo_root.clone());
        emit_progress(reporter, ProgressKind::Info, "preparing knots worktree")?;
        worktree.ensure_exists(&self.git)?;

        emit_progress(
            reporter,
            ProgressKind::Info,
            "scanning local knots event files",
        )?;
        let local_files = self.collect_local_event_files()?;
        let local_event_files = local_files.len() as u64;
        if local_event_files == 0 {
            emit_progress(
                reporter,
                ProgressKind::Success,
                "no local knots events found; nothing to push",
            )?;
            return Ok(PushSummary {
                local_event_files,
                copied_files: 0,
                committed: false,
                pushed: false,
                commit: None,
            });
        }

        for attempt in 0..MAX_ATTEMPTS {
            self.reset_worktree_to_remote_or_local(&worktree, reporter)?;
            worktree.ensure_clean(&self.git)?;

            emit_progress(
                reporter,
                ProgressKind::Info,
                format!("copying {local_event_files} local knot file(s) into the publish worktree"),
            )?;
            let copied_files = self.copy_files_into_worktree(worktree.path(), &local_files)?;
            let stage_paths = stage_paths(worktree.path());
            if stage_paths.is_empty() {
                emit_progress(
                    reporter,
                    ProgressKind::Success,
                    "remote knots already includes the local events",
                )?;
                return Ok(PushSummary {
                    local_event_files,
                    copied_files,
                    committed: false,
                    pushed: false,
                    commit: None,
                });
            }

            self.git.add_paths(worktree.path(), &stage_paths)?;

            if !self.git.has_staged_changes(worktree.path(), &stage_paths)? {
                emit_progress(
                    reporter,
                    ProgressKind::Success,
                    "remote knots already includes the local events",
                )?;
                return Ok(PushSummary {
                    local_event_files,
                    copied_files,
                    committed: false,
                    pushed: false,
                    commit: None,
                });
            }

            emit_progress(reporter, ProgressKind::Info, "creating a publish commit")?;
            let commit = self
                .git
                .commit(worktree.path(), "knots: publish local events")?;

            emit_progress(
                reporter,
                ProgressKind::Info,
                "pushing knots branch to origin",
            )?;
            match self
                .git
                .push_branch(worktree.path(), worktree.remote(), worktree.branch())
            {
                Ok(()) => {
                    emit_progress(
                        reporter,
                        ProgressKind::Success,
                        format!("push complete at {}", short_commit(&commit)),
                    )?;
                    return Ok(PushSummary {
                        local_event_files,
                        copied_files,
                        committed: true,
                        pushed: true,
                        commit: Some(commit),
                    });
                }
                Err(err) if err.is_non_fast_forward() && attempt + 1 < MAX_ATTEMPTS => {
                    emit_progress(
                        reporter,
                        ProgressKind::Warn,
                        format!(
                            "push was rejected; refreshing remote state and retrying ({}/{})",
                            attempt + 2,
                            MAX_ATTEMPTS
                        ),
                    )?;
                    continue;
                }
                Err(err) if err.is_non_fast_forward() => {
                    return Err(SyncError::MergeConflictEscalation {
                        message: "push rejected as non-fast-forward after retries".to_string(),
                    });
                }
                Err(err) => return Err(err),
            }
        }

        Err(SyncError::MergeConflictEscalation {
            message: "push retries exhausted".to_string(),
        })
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

    pub fn count_unpushed_event_files(&self) -> Result<u64, SyncError> {
        let worktree = KnotsWorktree::new(self.repo_root.clone());
        worktree.ensure_exists(&self.git)?;
        let mut reporter = None;
        self.reset_worktree_to_remote_or_local(&worktree, &mut reporter)?;
        worktree.ensure_clean(&self.git)?;

        let local_files = self.collect_local_event_files()?;
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
    ) -> Result<(), SyncError> {
        emit_progress(
            reporter,
            ProgressKind::Info,
            "refreshing knots worktree from origin/knots",
        )?;
        match self.git.fetch_branch_with_filter(
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
                Ok(())
            }
            Err(err) if err.is_missing_remote() || err.is_unknown_revision() => {
                emit_progress(
                    reporter,
                    ProgressKind::Warn,
                    "origin/knots is unavailable; using local knots worktree state",
                )?;
                let head = self.git.rev_parse(worktree.path(), "HEAD")?;
                self.git.reset_hard(worktree.path(), &head)?;
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    fn collect_local_event_files(&self) -> Result<Vec<PathBuf>, SyncError> {
        let mut files = Vec::new();
        for rel_root in [".knots/index", ".knots/events", ".knots/snapshots"] {
            let root = self.repo_root.join(rel_root);
            if !root.exists() {
                continue;
            }
            let mut stack = vec![root];
            while let Some(dir) = stack.pop() {
                for entry in std::fs::read_dir(&dir)? {
                    let path = entry?.path();
                    if path.is_dir() {
                        stack.push(path);
                        continue;
                    }
                    if path.extension().is_none_or(|ext| ext != "json") {
                        continue;
                    }
                    let relative = path
                        .strip_prefix(&self.repo_root)
                        .map_err(|err| SyncError::InvalidEvent {
                            path: path.clone(),
                            message: format!("failed to relativize event file: {}", err),
                        })?
                        .to_path_buf();
                    files.push(relative);
                }
            }
        }

        files.sort();
        Ok(files)
    }

    fn copy_files_into_worktree(
        &self,
        worktree_root: &Path,
        relative_files: &[PathBuf],
    ) -> Result<u64, SyncError> {
        let mut copied = 0u64;
        for relative in relative_files {
            let src = self.repo_root.join(relative);
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
            copied += 1;
        }

        Ok(copied)
    }

    fn event_file_missing_or_changed(
        &self,
        worktree_root: &Path,
        relative_file: &Path,
    ) -> Result<bool, SyncError> {
        let src = self.repo_root.join(relative_file);
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

    fn require_no_active_leases(&self) -> Result<(), SyncError> {
        let count = crate::db::count_active_leases(self.conn)?;
        if count > 0 {
            return Err(SyncError::ActiveLeasesExist(count));
        }
        Ok(())
    }
}

fn short_commit(commit: &str) -> &str {
    &commit[..commit.len().min(12)]
}

fn stage_paths(worktree_root: &Path) -> Vec<&'static str> {
    let mut out = Vec::new();
    for path in [".knots/index", ".knots/events", ".knots/snapshots"] {
        if worktree_root.join(path).exists() {
            out.push(path);
        }
    }
    out
}

#[cfg(test)]
mod tests;
