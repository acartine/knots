use std::path::{Path, PathBuf};

use super::{GitAdapter, SyncError};

#[derive(Debug, Clone)]
pub struct KnotsWorktree {
    root: PathBuf,
    path: PathBuf,
    branch: String,
    remote: String,
}

impl KnotsWorktree {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            path: root.join(".knots").join("_worktree"),
            root,
            branch: "knots".to_string(),
            remote: "origin".to_string(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn branch(&self) -> &str {
        &self.branch
    }

    pub fn remote(&self) -> &str {
        &self.remote
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

        if git.branch_exists(&self.root, &self.branch)? {
            git.worktree_add_existing_branch(&self.root, &self.path, &self.branch)?;
        } else {
            git.worktree_add_new_branch(&self.root, &self.path, &self.branch)?;
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
        if current == self.branch {
            return Ok(());
        }
        git.checkout_branch(&self.path, &self.branch)
    }
}
