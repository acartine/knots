use std::path::Path;
use std::process::Command;

#[derive(Debug)]
pub enum RemoteInitError {
    NotGitRepository,
    MissingRemote(String),
    RemoteBranchExists {
        remote: String,
        branch: String,
    },
    GitCommandFailed {
        command: String,
        code: Option<i32>,
        stderr: String,
    },
    Io(std::io::Error),
}

impl std::fmt::Display for RemoteInitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RemoteInitError::NotGitRepository => write!(f, "not a git repository"),
            RemoteInitError::MissingRemote(remote) => {
                write!(f, "git remote '{}' is not configured", remote)
            }
            RemoteInitError::RemoteBranchExists { remote, branch } => {
                write!(f, "remote branch '{}/{}' already exists", remote, branch)
            }
            RemoteInitError::GitCommandFailed {
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
            RemoteInitError::Io(err) => write!(f, "I/O error: {}", err),
        }
    }
}

impl std::error::Error for RemoteInitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RemoteInitError::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for RemoteInitError {
    fn from(value: std::io::Error) -> Self {
        RemoteInitError::Io(value)
    }
}

pub fn init_remote_knots_branch(repo_root: &Path) -> Result<(), RemoteInitError> {
    init_remote_branch(repo_root, "origin", "knots")
}

fn init_remote_branch(repo_root: &Path, remote: &str, branch: &str) -> Result<(), RemoteInitError> {
    if !repo_root.join(".git").exists() {
        return Err(RemoteInitError::NotGitRepository);
    }

    ensure_remote_exists(repo_root, remote)?;
    ensure_remote_branch_missing(repo_root, remote, branch)?;

    if !local_branch_exists(repo_root, branch)? {
        run_checked(repo_root, &["branch", branch])?;
    }

    run_checked(
        repo_root,
        &["push", "-u", remote, &format!("{}:{}", branch, branch)],
    )?;
    Ok(())
}

fn ensure_remote_exists(repo_root: &Path, remote: &str) -> Result<(), RemoteInitError> {
    let output = run(repo_root, &["remote", "get-url", remote])?;
    if output.status.success() {
        return Ok(());
    }
    Err(RemoteInitError::MissingRemote(remote.to_string()))
}

fn ensure_remote_branch_missing(
    repo_root: &Path,
    remote: &str,
    branch: &str,
) -> Result<(), RemoteInitError> {
    let output = run(
        repo_root,
        &["ls-remote", "--exit-code", "--heads", remote, branch],
    )?;
    if output.status.success() {
        return Err(RemoteInitError::RemoteBranchExists {
            remote: remote.to_string(),
            branch: branch.to_string(),
        });
    }

    if output.status.code() == Some(2) {
        return Ok(());
    }

    Err(command_failure(
        repo_root,
        &["ls-remote", "--exit-code", "--heads", remote, branch],
        output,
    ))
}

fn local_branch_exists(repo_root: &Path, branch: &str) -> Result<bool, RemoteInitError> {
    let output = run(
        repo_root,
        &["show-ref", "--verify", &format!("refs/heads/{}", branch)],
    )?;
    Ok(output.status.success())
}

fn run_checked(repo_root: &Path, args: &[&str]) -> Result<String, RemoteInitError> {
    let output = run(repo_root, args)?;
    if !output.status.success() {
        return Err(command_failure(repo_root, args, output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn run(repo_root: &Path, args: &[&str]) -> Result<std::process::Output, RemoteInitError> {
    Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .map_err(RemoteInitError::Io)
}

fn command_failure(
    repo_root: &Path,
    args: &[&str],
    output: std::process::Output,
) -> RemoteInitError {
    RemoteInitError::GitCommandFailed {
        command: format!("git -C {} {}", repo_root.display(), args.join(" ")),
        code: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use uuid::Uuid;

    use super::{init_remote_branch, RemoteInitError};

    fn unique_dir(prefix: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("{}-{}", prefix, Uuid::now_v7()));
        std::fs::create_dir_all(&path).expect("temp dir should be creatable");
        path
    }

    fn run_git(cwd: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(cwd)
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

    fn setup_repo_with_remote() -> (PathBuf, PathBuf) {
        let root = unique_dir("knots-init-remote-test");
        let remote = root.join("remote.git");
        let local = root.join("local");

        std::fs::create_dir_all(&local).expect("local dir should be creatable");
        run_git(
            &root,
            &["init", "--bare", remote.to_str().expect("utf8 path")],
        );

        run_git(&local, &["init"]);
        run_git(&local, &["config", "user.email", "knots@example.com"]);
        run_git(&local, &["config", "user.name", "Knots Test"]);
        std::fs::write(local.join("README.md"), "# knots\n").expect("readme should write");
        run_git(&local, &["add", "README.md"]);
        run_git(&local, &["commit", "-m", "init"]);
        run_git(
            &local,
            &[
                "remote",
                "add",
                "origin",
                remote.to_str().expect("utf8 path"),
            ],
        );

        (root, local)
    }

    #[test]
    fn creates_remote_branch_when_missing() {
        let (root, local) = setup_repo_with_remote();

        init_remote_branch(&local, "origin", "knots").expect("init remote should succeed");

        let output = Command::new("git")
            .arg("-C")
            .arg(&local)
            .args(["ls-remote", "--heads", "origin", "knots"])
            .output()
            .expect("git ls-remote should run");
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("refs/heads/knots"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn fails_if_remote_branch_exists() {
        let (root, local) = setup_repo_with_remote();
        init_remote_branch(&local, "origin", "knots").expect("first init should succeed");

        let second = init_remote_branch(&local, "origin", "knots");
        assert!(matches!(
            second,
            Err(RemoteInitError::RemoteBranchExists { .. })
        ));

        let _ = std::fs::remove_dir_all(root);
    }
}
