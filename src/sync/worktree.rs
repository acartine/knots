use std::path::{Path, PathBuf};

use crate::project::StorePaths;
use crate::sync_ref::SyncRefConfig;

use super::{GitAdapter, SyncError};

#[derive(Debug, Clone)]
pub struct KnotsWorktree {
    root: PathBuf,
    path: PathBuf,
    config: SyncRefConfig,
}

impl KnotsWorktree {
    #[cfg(test)]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let store_paths = StorePaths {
            root: root.join(".knots"),
        };
        Self::with_store_paths(root, &store_paths)
    }

    pub fn with_store_paths(root: impl Into<PathBuf>, store_paths: &StorePaths) -> Self {
        let root = root.into();
        let config = SyncRefConfig::for_repo(&root);
        Self {
            path: store_paths.worktree_path(),
            root,
            config,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn branch(&self) -> &str {
        self.config.local_branch()
    }

    pub fn remote(&self) -> &str {
        self.config.remote()
    }

    pub fn fetch_refspec(&self) -> String {
        self.config.fetch_refspec()
    }

    pub fn push_refspec(&self) -> String {
        self.config.push_refspec()
    }

    pub fn tracking_rev(&self) -> String {
        self.config.tracking_rev()
    }

    pub fn remote_display(&self) -> String {
        self.config.remote_display()
    }

    pub fn ensure_exists(&self, git: &GitAdapter) -> Result<(), SyncError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if self.path.join(".git").exists() {
            self.ensure_branch_checked_out(git)?;
            return Ok(());
        }

        if self.path.exists() {
            return Err(SyncError::DirtyWorktree(self.path.clone()));
        }

        if git.branch_exists(&self.root, self.branch())? {
            git.worktree_add_existing_branch(&self.root, &self.path, self.branch())?;
        } else {
            git.worktree_add_new_branch(&self.root, &self.path, self.branch())?;
        }

        self.ensure_branch_checked_out(git)
    }

    pub fn ensure_clean(&self, git: &GitAdapter) -> Result<(), SyncError> {
        if git.status_clean(&self.path)? {
            Ok(())
        } else {
            Err(SyncError::DirtyWorktree(self.path.clone()))
        }
    }

    fn ensure_branch_checked_out(&self, git: &GitAdapter) -> Result<(), SyncError> {
        let current = git.current_branch(&self.path)?;
        if current == self.branch() {
            return Ok(());
        }
        git.checkout_branch(&self.path, self.branch())
    }
}
