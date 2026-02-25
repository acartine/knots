use std::path::{Path, PathBuf};
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

const KNOWN_GIT_HOOKS: &[&str] = &[
    "applypatch-msg",
    "commit-msg",
    "pre-applypatch",
    "pre-commit",
    "pre-merge-commit",
    "prepare-commit-msg",
    "pre-push",
    "pre-rebase",
    "pre-receive",
    "update",
];

const BD_BEADS_SIGNATURES: &[&str] = &["bd ", "bd\n", "beads", "uncommitted changes detected"];

#[derive(Debug, Clone)]
pub struct BeadsHookReport {
    pub hooks_dir: PathBuf,
    pub hook_files: Vec<PathBuf>,
    pub has_beads_config: bool,
}

impl BeadsHookReport {
    pub fn is_empty(&self) -> bool {
        self.hook_files.is_empty() && !self.has_beads_config
    }
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

pub fn detect_beads_hooks(repo_root: &Path) -> BeadsHookReport {
    let mut hooks_dir = repo_root.join(".git").join("hooks");
    if let Ok(output) = run(
        repo_root,
        &["config", "--local", "--get", "core.hooksPath"],
    ) {
        if output.status.success() {
            let configured = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !configured.is_empty() {
                let path = Path::new(&configured);
                if path.is_absolute() {
                    hooks_dir = path.to_path_buf();
                } else {
                    hooks_dir = repo_root.join(path);
                }
            }
        }
    }

    let mut report = BeadsHookReport {
        hooks_dir,
        hook_files: Vec::new(),
        has_beads_config: has_beads_config_entry(repo_root),
    };

    if !report.hooks_dir.exists() {
        return report;
    }

    for hook in KNOWN_GIT_HOOKS {
        let path = report.hooks_dir.join(hook);
        if !path.exists() {
            continue;
        }
        if path_is_beads_related(&path) {
            report.hook_files.push(path);
        }
    }

    report
}

pub fn remote_branch_exists(
    repo_root: &Path,
    remote: &str,
    branch: &str,
) -> Result<bool, RemoteInitError> {
    let output = run(repo_root, &["ls-remote", "--exit-code", "--heads", remote, branch])?;
    if output.status.success() {
        return Ok(true);
    }

    if output.status.code() == Some(2) {
        return Ok(false);
    }

    Err(command_failure(
        repo_root,
        &["ls-remote", "--exit-code", "--heads", remote, branch],
        output,
    ))
}

pub fn uninit_remote_knots_branch(
    repo_root: &Path,
    remote: &str,
    branch: &str,
) -> Result<bool, RemoteInitError> {
    if !repo_root.join(".git").exists() {
        return Err(RemoteInitError::NotGitRepository);
    }

    ensure_remote_exists(repo_root, remote)?;
    if !remote_branch_exists(repo_root, remote, branch)? {
        return Ok(false);
    }
    run_push_with_hook_fallback(repo_root, &["push", "--delete", remote, branch])?;
    Ok(true)
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

    let push_spec = format!("{}:{}", branch, branch);
    let push_args = ["push", "-u", remote, push_spec.as_str()];
    run_push_with_hook_fallback(repo_root, &push_args)?;
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

fn run_push_with_hook_fallback(
    repo_root: &Path,
    args: &[&str],
) -> Result<String, RemoteInitError> {
    match run_checked(repo_root, args) {
        Ok(out) => Ok(out),
        Err(err) if should_retry_push_without_verify(&err) => {
            let mut no_verify_args = Vec::with_capacity(args.len() + 1);
            no_verify_args.push("push");
            if !args.is_empty() && args[0] == "push" {
                no_verify_args.push("--no-verify");
                no_verify_args.extend(args.iter().skip(1).copied());
                run_checked(repo_root, &no_verify_args)
            } else {
                Err(err)
            }
        }
        Err(err) => Err(err),
    }
}

fn should_retry_push_without_verify(error: &RemoteInitError) -> bool {
    match error {
        RemoteInitError::GitCommandFailed {
            code: Some(1),
            stderr,
            ..
        } => {
            let stderr = stderr.to_lowercase();
            has_bd_beads_signature(&stderr)
        }
        _ => false,
    }
}

fn has_bd_beads_signature(line: &str) -> bool {
    BD_BEADS_SIGNATURES.iter().any(|sig| line.contains(sig))
}

fn path_is_beads_related(path: &Path) -> bool {
    if let Ok(contents) = std::fs::read_to_string(path) {
        has_bd_beads_signature(&contents.to_lowercase())
    } else {
        false
    }
}

fn has_beads_config_entry(repo_root: &Path) -> bool {
    if let Ok(output) = run(
        repo_root,
        &["config", "--local", "--get-regexp", "^beads\\..*"],
    ) {
        output.status.success() && !String::from_utf8_lossy(&output.stdout).trim().is_empty()
    } else {
        false
    }
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

    use super::{detect_beads_hooks, init_remote_branch, RemoteInitError};

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

    #[test]
    fn detects_beads_hook_in_hook_file() {
        let (root, local) = setup_repo_with_remote();
        let hooks = local.join(".git").join("hooks");
        std::fs::create_dir_all(&hooks).expect("hooks dir should be creatable");

        let pre_push = hooks.join("pre-push");
        std::fs::write(&pre_push, "#!/bin/sh\nbd sync\n")
            .expect("pre-push hook should be writable");
        let report = detect_beads_hooks(&local);
        assert!(!report.hook_files.is_empty());
        assert!(report.hook_files.contains(&pre_push));
        assert!(!report.has_beads_config);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn detects_beads_config() {
        let (root, local) = setup_repo_with_remote();
        run_git(&local, &["config", "beads.role", "maintainer"]);
        let report = detect_beads_hooks(&local);
        assert!(report.has_beads_config);

        let _ = std::fs::remove_dir_all(root);
    }
}
