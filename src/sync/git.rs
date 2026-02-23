use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::SyncError;

#[derive(Debug, Clone, Default)]
pub struct GitAdapter;

impl GitAdapter {
    pub fn new() -> Self {
        Self
    }

    pub fn fetch_branch(
        &self,
        repo_root: &Path,
        remote: &str,
        branch: &str,
    ) -> Result<(), SyncError> {
        self.run_checked(
            repo_root,
            vec![
                "fetch".to_string(),
                "--no-tags".to_string(),
                "--prune".to_string(),
                remote.to_string(),
                branch.to_string(),
            ],
        )?;
        Ok(())
    }

    pub fn rev_parse(&self, cwd: &Path, rev: &str) -> Result<String, SyncError> {
        self.run_checked(cwd, vec!["rev-parse".to_string(), rev.to_string()])
    }

    pub fn reset_hard(&self, cwd: &Path, rev: &str) -> Result<(), SyncError> {
        self.run_checked(
            cwd,
            vec!["reset".to_string(), "--hard".to_string(), rev.to_string()],
        )?;
        Ok(())
    }

    pub fn status_clean(&self, cwd: &Path) -> Result<bool, SyncError> {
        let output =
            self.run_checked(cwd, vec!["status".to_string(), "--porcelain".to_string()])?;
        Ok(output.trim().is_empty())
    }

    pub fn branch_exists(&self, cwd: &Path, branch: &str) -> Result<bool, SyncError> {
        let output = self.run_allow_failure(
            cwd,
            vec![
                "show-ref".to_string(),
                "--verify".to_string(),
                format!("refs/heads/{}", branch),
            ],
        )?;
        Ok(output.status.success())
    }

    pub fn current_branch(&self, cwd: &Path) -> Result<String, SyncError> {
        self.run_checked(
            cwd,
            vec![
                "rev-parse".to_string(),
                "--abbrev-ref".to_string(),
                "HEAD".to_string(),
            ],
        )
    }

    pub fn checkout_branch(&self, cwd: &Path, branch: &str) -> Result<(), SyncError> {
        self.run_checked(cwd, vec!["checkout".to_string(), branch.to_string()])?;
        Ok(())
    }

    pub fn worktree_add_existing_branch(
        &self,
        repo_root: &Path,
        worktree: &Path,
        branch: &str,
    ) -> Result<(), SyncError> {
        self.run_checked(
            repo_root,
            vec![
                "worktree".to_string(),
                "add".to_string(),
                "--force".to_string(),
                display_path(worktree),
                branch.to_string(),
            ],
        )?;
        Ok(())
    }

    pub fn worktree_add_new_branch(
        &self,
        repo_root: &Path,
        worktree: &Path,
        branch: &str,
    ) -> Result<(), SyncError> {
        self.run_checked(
            repo_root,
            vec![
                "worktree".to_string(),
                "add".to_string(),
                "-B".to_string(),
                branch.to_string(),
                display_path(worktree),
            ],
        )?;
        Ok(())
    }

    pub fn diff_name_only(
        &self,
        cwd: &Path,
        from: &str,
        to: &str,
        pathspec: &str,
    ) -> Result<Vec<PathBuf>, SyncError> {
        let stdout = self.run_checked(
            cwd,
            vec![
                "diff".to_string(),
                "--name-only".to_string(),
                "--diff-filter=AM".to_string(),
                format!("{}..{}", from, to),
                "--".to_string(),
                pathspec.to_string(),
            ],
        )?;
        Ok(parse_lines(&stdout))
    }

    fn run_checked(&self, cwd: &Path, args: Vec<String>) -> Result<String, SyncError> {
        let output = self.run_allow_failure(cwd, args.clone())?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(SyncError::GitCommandFailed {
                command: display_command(cwd, &args),
                code: output.status.code(),
                stderr,
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn run_allow_failure(&self, cwd: &Path, args: Vec<String>) -> Result<Output, SyncError> {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(cwd).args(&args);
        cmd.output().map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                SyncError::GitUnavailable
            } else {
                SyncError::Io(err)
            }
        })
    }
}

fn parse_lines(value: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for line in value.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            out.push(PathBuf::from(trimmed));
        }
    }
    out
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn display_command(cwd: &Path, args: &[String]) -> String {
    format!("git -C {} {}", cwd.display(), args.join(" "))
}
