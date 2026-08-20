use std::path::{Path, PathBuf};
use std::process::Command;

use crate::project::canonical_or_original;

/// Resolve the directory under which the shared Knots store should live.
///
/// In a Git linked worktree, `.git` is a file pointing into
/// `<primary>/.git/worktrees/<name>`. Knots state must be shared across
/// linked worktrees so post-merge sync does not try to re-check-out the
/// `knots` branch into a per-worktree clone of `.knots/_worktree`. When
/// `repo_root` is a linked worktree, this returns the primary worktree
/// root; otherwise it returns `repo_root` unchanged.
pub fn store_root_base_for(repo_root: &Path) -> PathBuf {
    if repo_root.join(".git").is_file() {
        if let Some(primary) = primary_worktree_root(repo_root) {
            return primary;
        }
    }
    repo_root.to_path_buf()
}

/// Resolve the primary worktree root for a Git repo path. Returns `None`
/// when the common dir cannot be resolved or does not point at a `.git`
/// directory.
fn primary_worktree_root(repo_root: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["rev-parse", "--git-common-dir"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = std::str::from_utf8(&output.stdout).ok()?.trim();
    if raw.is_empty() {
        return None;
    }
    let mut common_dir = PathBuf::from(raw);
    if common_dir.is_relative() {
        common_dir = repo_root.join(common_dir);
    }
    let common_dir = canonical_or_original(&common_dir);
    if common_dir.file_name()?.to_str()? != ".git" {
        return None;
    }
    common_dir.parent().map(canonical_or_original)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("git command should run");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn temp_workspace(prefix: &str) -> knots_test_support::TestWorkspace {
        let path_ws = knots_test_support::workspace(prefix);
        let _path = path_ws.path().to_path_buf();
        path_ws
    }

    fn seed_repo(primary: &Path) {
        std::fs::create_dir_all(primary).expect("primary dir");
        run_git(primary, &["init"]);
        run_git(primary, &["config", "user.email", "knots@example.com"]);
        run_git(primary, &["config", "user.name", "Knots Test"]);
        run_git(primary, &["config", "commit.gpgsign", "false"]);
        std::fs::write(primary.join("README.md"), "x").expect("seed file");
        run_git(primary, &["add", "README.md"]);
        run_git(primary, &["commit", "-m", "init"]);
    }

    #[test]
    fn primary_worktree_root_returns_primary_for_linked_worktree() {
        let root_ws = temp_workspace("knots-project-worktree");
        let root = root_ws.path().to_path_buf();
        let primary = root.join("primary");
        seed_repo(&primary);
        let linked = root.join("linked");
        run_git(
            &primary,
            &[
                "worktree",
                "add",
                linked.to_str().expect("utf8 linked path"),
                "-b",
                "feature",
            ],
        );

        let from_linked = primary_worktree_root(&linked).expect("primary worktree from linked");
        let expected = canonical_or_original(&primary);
        assert_eq!(from_linked, expected);

        let from_primary = primary_worktree_root(&primary).expect("primary worktree from primary");
        assert_eq!(from_primary, expected);
    }

    #[test]
    fn primary_worktree_root_returns_none_outside_git() {
        let root_ws = temp_workspace("knots-project-worktree-nogit");
        let root = root_ws.path().to_path_buf();
        assert!(primary_worktree_root(&root).is_none());
    }
}
