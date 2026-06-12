use std::path::Path;
use std::process::Command;

pub const DEFAULT_REMOTE: &str = "origin";
pub const DEFAULT_LOCAL_BRANCH: &str = "knots";
pub const LEGACY_REMOTE_REF: &str = "refs/heads/knots";
pub const DIFFINITE_REMOTE_REF: &str = "refs/work/knots";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRefConfig {
    remote: String,
    local_branch: String,
    remote_ref: String,
}

impl SyncRefConfig {
    pub fn for_repo(repo_root: &Path) -> Self {
        let remote =
            git_config(repo_root, "knots.remote").unwrap_or_else(|| DEFAULT_REMOTE.to_string());
        let local_branch = git_config(repo_root, "knots.localBranch")
            .unwrap_or_else(|| DEFAULT_LOCAL_BRANCH.to_string());
        let remote_ref = git_config(repo_root, "knots.remoteRef")
            .or_else(|| std::env::var("KNOTS_REMOTE_REF").ok())
            .unwrap_or_else(|| default_remote_ref(repo_root, &remote));
        Self {
            remote,
            local_branch,
            remote_ref: normalize_ref(&remote_ref),
        }
    }

    pub fn remote(&self) -> &str {
        &self.remote
    }

    pub fn local_branch(&self) -> &str {
        &self.local_branch
    }

    pub fn remote_ref(&self) -> &str {
        &self.remote_ref
    }

    pub fn tracking_ref(&self) -> String {
        format!("refs/remotes/{}/{}", self.remote, self.local_branch)
    }

    pub fn tracking_rev(&self) -> String {
        format!("{}/{}", self.remote, self.local_branch)
    }

    pub fn fetch_refspec(&self) -> String {
        format!("+{}:{}", self.remote_ref, self.tracking_ref())
    }

    pub fn push_refspec(&self) -> String {
        format!("HEAD:{}", self.remote_ref)
    }

    pub fn remote_display(&self) -> String {
        format!("{}:{}", self.remote, self.remote_ref)
    }
}

pub fn write_remote_ref_override(repo_root: &Path, remote_ref: &str) -> std::io::Result<()> {
    git_config_set(repo_root, "knots.remoteRef", &normalize_ref(remote_ref))
}

pub fn write_sync_ref_config(
    repo_root: &Path,
    remote: &str,
    remote_ref: &str,
) -> std::io::Result<()> {
    git_config_set(repo_root, "knots.remote", remote)?;
    write_remote_ref_override(repo_root, remote_ref)
}

fn git_config_set(repo_root: &Path, key: &str, value: &str) -> std::io::Result<()> {
    let status = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["config", key, value])
        .status()?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| std::io::Error::other(format!("git config {key} failed")))
}

fn default_remote_ref(repo_root: &Path, remote: &str) -> String {
    match remote_url(repo_root, remote) {
        Some(url) if url.contains("diffinite.sneka.ai") => DIFFINITE_REMOTE_REF.to_string(),
        _ => LEGACY_REMOTE_REF.to_string(),
    }
}

pub(crate) fn normalize_ref(value: &str) -> String {
    if value.starts_with("refs/") {
        value.to_string()
    } else {
        format!("refs/heads/{value}")
    }
}

fn git_config(repo_root: &Path, key: &str) -> Option<String> {
    git_output(repo_root, &["config", "--get", key])
}

fn remote_url(repo_root: &Path, remote: &str) -> Option<String> {
    git_output(repo_root, &["remote", "get-url", remote])
}

fn git_output(repo_root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{normalize_ref, SyncRefConfig, DIFFINITE_REMOTE_REF, LEGACY_REMOTE_REF};
    use std::path::Path;
    use std::process::Command;
    use uuid::Uuid;

    #[test]
    fn normalizes_short_branch_to_heads_ref() {
        assert_eq!(normalize_ref("knots"), LEGACY_REMOTE_REF);
        assert_eq!(normalize_ref(DIFFINITE_REMOTE_REF), DIFFINITE_REMOTE_REF);
    }

    #[test]
    fn diffinite_origin_defaults_to_work_ref() {
        let repo = temp_repo();
        git(
            &repo,
            &["remote", "add", "origin", "git@diffinite.sneka.ai:o/r.git"],
        );
        let config = SyncRefConfig::for_repo(&repo);
        assert_eq!(config.remote_ref(), DIFFINITE_REMOTE_REF);
        assert_eq!(
            config.fetch_refspec(),
            "+refs/work/knots:refs/remotes/origin/knots"
        );
        assert_eq!(config.push_refspec(), "HEAD:refs/work/knots");
    }

    #[test]
    fn github_origin_defaults_to_legacy_branch() {
        let repo = temp_repo();
        git(
            &repo,
            &["remote", "add", "origin", "git@github.com:o/r.git"],
        );
        let config = SyncRefConfig::for_repo(&repo);
        assert_eq!(config.remote_ref(), LEGACY_REMOTE_REF);
    }

    fn temp_repo() -> std::path::PathBuf {
        let repo = std::env::temp_dir().join(format!("knots-sync-ref-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&repo).expect("create temp repo");
        git(&repo, &["init"]);
        repo
    }

    fn git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
