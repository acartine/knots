use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::Serialize;

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
        let service = SyncService::new(self.conn, self.repo_root.clone());
        service.sync()
    }

    pub fn push(&self) -> Result<PushSummary, SyncError> {
        const MAX_ATTEMPTS: usize = 3;

        let worktree = KnotsWorktree::new(self.repo_root.clone());
        worktree.ensure_exists(&self.git)?;

        let local_files = self.collect_local_event_files()?;
        let local_event_files = local_files.len() as u64;

        for attempt in 0..MAX_ATTEMPTS {
            self.reset_worktree_to_remote_or_local(&worktree)?;
            worktree.ensure_clean(&self.git)?;

            let copied_files = self.copy_files_into_worktree(worktree.path(), &local_files)?;
            self.git
                .add_paths(worktree.path(), &[".knots/index", ".knots/events"])?;

            if !self
                .git
                .has_staged_changes(worktree.path(), &[".knots/index", ".knots/events"])?
            {
                return Ok(PushSummary {
                    local_event_files,
                    copied_files,
                    committed: false,
                    pushed: false,
                    commit: None,
                });
            }

            let commit = self
                .git
                .commit(worktree.path(), "knots: publish local events")?;

            match self
                .git
                .push_branch(worktree.path(), worktree.remote(), worktree.branch())
            {
                Ok(()) => {
                    return Ok(PushSummary {
                        local_event_files,
                        copied_files,
                        committed: true,
                        pushed: true,
                        commit: Some(commit),
                    });
                }
                Err(err) if err.is_non_fast_forward() && attempt + 1 < MAX_ATTEMPTS => continue,
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
        let push = self.push()?;
        let pull = self.pull()?;
        Ok(ReplicationSummary { push, pull })
    }

    fn reset_worktree_to_remote_or_local(&self, worktree: &KnotsWorktree) -> Result<(), SyncError> {
        match self
            .git
            .fetch_branch(&self.repo_root, worktree.remote(), worktree.branch())
        {
            Ok(()) => {
                let remote_ref = format!("{}/{}", worktree.remote(), worktree.branch());
                let head = self.git.rev_parse(&self.repo_root, &remote_ref)?;
                self.git.reset_hard(worktree.path(), &head)?;
                Ok(())
            }
            Err(err) if err.is_missing_remote() || err.is_unknown_revision() => {
                let head = self.git.rev_parse(worktree.path(), "HEAD")?;
                self.git.reset_hard(worktree.path(), &head)?;
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    fn collect_local_event_files(&self) -> Result<Vec<PathBuf>, SyncError> {
        let mut files = Vec::new();
        for rel_root in [".knots/index", ".knots/events"] {
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
                    if !path.extension().is_some_and(|ext| ext == "json") {
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
}

#[cfg(test)]
mod tests;
