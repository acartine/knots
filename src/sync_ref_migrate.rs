use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::app::AppError;
use crate::cli_sync_ref::{SyncRefArgs, SyncRefSubcommands};
use crate::sync_ref::{normalize_ref, write_sync_ref_config, SyncRefConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
struct RefEndpoint {
    remote: String,
    refname: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    Local,
    Remote(RefEndpoint),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MigrationSummary {
    files: usize,
    commit: Option<String>,
    target: RefEndpoint,
}

pub fn run_sync_ref_command(repo_root: &Path, args: &SyncRefArgs) -> Result<(), AppError> {
    match &args.command {
        SyncRefSubcommands::Migrate(args) => {
            let summary = migrate(repo_root, &args.sources, args.target.as_deref())?;
            println!("sync_ref_migrate=ok");
            println!(
                "target={}:{}",
                summary.target.remote, summary.target.refname
            );
            println!("files={}", summary.files);
            match summary.commit {
                Some(commit) => println!("commit={commit}"),
                None => println!("commit=unchanged"),
            }
            Ok(())
        }
    }
}

fn migrate(
    repo_root: &Path,
    raw_sources: &[String],
    raw_target: Option<&str>,
) -> Result<MigrationSummary, AppError> {
    ensure_git_repo(repo_root)?;
    let target = parse_target(repo_root, raw_target)?;
    let mut files = BTreeMap::new();

    harvest_remote(repo_root, &target, &mut files, MissingRemoteRef::Skip)?;
    for raw in raw_sources {
        match parse_source(raw)? {
            Source::Local => harvest_local(repo_root, &mut files)?,
            Source::Remote(endpoint) => {
                ensure_remote_exists(repo_root, &endpoint.remote)?;
                harvest_remote(repo_root, &endpoint, &mut files, MissingRemoteRef::Skip)?;
            }
        }
    }

    if files.is_empty() {
        return Err(AppError::InvalidArgument(
            "no Knots event, index, or snapshot files found to migrate".to_string(),
        ));
    }

    let commit = publish_target(repo_root, &target, &files)?;
    write_sync_ref_config(repo_root, &target.remote, &target.refname)?;
    Ok(MigrationSummary {
        files: files.len(),
        commit,
        target,
    })
}

fn parse_target(repo_root: &Path, raw: Option<&str>) -> Result<RefEndpoint, AppError> {
    if let Some(raw) = raw {
        return parse_endpoint(raw);
    }
    let config = SyncRefConfig::for_repo(repo_root);
    Ok(RefEndpoint {
        remote: config.remote().to_string(),
        refname: config.remote_ref().to_string(),
    })
}

fn parse_source(raw: &str) -> Result<Source, AppError> {
    if raw == "local" {
        Ok(Source::Local)
    } else {
        parse_endpoint(raw).map(Source::Remote)
    }
}

fn parse_endpoint(raw: &str) -> Result<RefEndpoint, AppError> {
    let Some((remote, refname)) = raw.split_once(':') else {
        return Err(AppError::InvalidArgument(format!(
            "expected '<remote>:<ref>' or 'local', got '{raw}'"
        )));
    };
    if remote.is_empty() || refname.is_empty() {
        return Err(AppError::InvalidArgument(format!(
            "expected '<remote>:<ref>' or 'local', got '{raw}'"
        )));
    }
    Ok(RefEndpoint {
        remote: remote.to_string(),
        refname: normalize_ref(refname),
    })
}

fn harvest_local(repo_root: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) -> Result<(), AppError> {
    let store_root = repo_root.join(".knots");
    for prefix in ["index", "events", "snapshots"] {
        let root = store_root.join(prefix);
        if root.exists() {
            harvest_local_dir(&store_root, &root, files)?;
        }
    }
    Ok(())
}

fn harvest_local_dir(
    store_root: &Path,
    root: &Path,
    files: &mut BTreeMap<PathBuf, Vec<u8>>,
) -> Result<(), AppError> {
    let mut stack = vec![root.to_path_buf()];
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
                .strip_prefix(store_root)
                .map_err(|err| AppError::InvalidArgument(err.to_string()))?;
            let relative = Path::new(".knots").join(relative);
            add_file(files, relative, std::fs::read(&path)?)?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MissingRemoteRef {
    Skip,
}

fn harvest_remote(
    repo_root: &Path,
    endpoint: &RefEndpoint,
    files: &mut BTreeMap<PathBuf, Vec<u8>>,
    missing: MissingRemoteRef,
) -> Result<(), AppError> {
    ensure_remote_exists(repo_root, &endpoint.remote)?;
    if !remote_ref_exists(repo_root, endpoint)? {
        match missing {
            MissingRemoteRef::Skip => return Ok(()),
        }
    }
    git_checked(
        repo_root,
        &["fetch", "--no-tags", &endpoint.remote, &endpoint.refname],
    )?;
    let paths = git_checked_bytes(
        repo_root,
        &[
            "ls-tree",
            "-r",
            "--name-only",
            "FETCH_HEAD",
            "--",
            ".knots/index",
            ".knots/events",
            ".knots/snapshots",
        ],
    )?;
    for path in String::from_utf8_lossy(&paths)
        .lines()
        .filter(|p| !p.is_empty())
    {
        let contents = git_checked_bytes(repo_root, &["show", &format!("FETCH_HEAD:{path}")])?;
        add_file(files, PathBuf::from(path), contents)?;
    }
    Ok(())
}

fn add_file(
    files: &mut BTreeMap<PathBuf, Vec<u8>>,
    path: PathBuf,
    contents: Vec<u8>,
) -> Result<(), AppError> {
    if let Some(existing) = files.get(&path) {
        if existing != &contents {
            return Err(AppError::InvalidArgument(format!(
                "Knots migration conflict at {}",
                path.display()
            )));
        }
        return Ok(());
    }
    files.insert(path, contents);
    Ok(())
}

fn publish_target(
    repo_root: &Path,
    target: &RefEndpoint,
    files: &BTreeMap<PathBuf, Vec<u8>>,
) -> Result<Option<String>, AppError> {
    let remote_url = git_checked(repo_root, &["remote", "get-url", &target.remote])?;
    let work_root =
        std::env::temp_dir().join(format!("knots-sync-ref-migrate-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&work_root)?;
    let publish = work_root.join("publish");
    git_checked(
        repo_root,
        &["clone", "--no-checkout", &remote_url, path_str(&publish)?],
    )?;

    let target_exists = remote_ref_exists(repo_root, target)?;
    if target_exists {
        git_checked(&publish, &["fetch", "--no-tags", "origin", &target.refname])?;
        git_checked(&publish, &["checkout", "-B", "knots-migrate", "FETCH_HEAD"])?;
    } else {
        git_checked(&publish, &["checkout", "--orphan", "knots-migrate"])?;
        let _ = git_output(&publish, &["rm", "-r", "--ignore-unmatch", "."])?;
    }

    git_checked(&publish, &["config", "user.email", "knots@example.com"])?;
    git_checked(&publish, &["config", "user.name", "Knots"])?;
    replace_knots_dirs(&publish, files)?;
    git_checked(&publish, &["add", "-A", "-f", "--", ".knots"])?;

    let changed = !git_output(&publish, &["diff", "--cached", "--quiet"])?
        .status
        .success();
    if changed {
        git_checked(&publish, &["commit", "-m", "knots: migrate sync ref"])?;
    }
    let commit = git_checked(&publish, &["rev-parse", "HEAD"])?;
    git_checked(
        &publish,
        &[
            "push",
            "--no-verify",
            "origin",
            &format!("HEAD:{}", target.refname),
        ],
    )?;
    let _ = std::fs::remove_dir_all(work_root);
    Ok(changed.then_some(commit))
}

fn replace_knots_dirs(publish: &Path, files: &BTreeMap<PathBuf, Vec<u8>>) -> Result<(), AppError> {
    for dir in [".knots/index", ".knots/events", ".knots/snapshots"] {
        let path = publish.join(dir);
        if path.exists() {
            std::fs::remove_dir_all(path)?;
        }
    }
    for (relative, contents) in files {
        let path = publish.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, contents)?;
    }
    Ok(())
}

fn ensure_git_repo(repo_root: &Path) -> Result<(), AppError> {
    if repo_root.join(".git").exists() {
        Ok(())
    } else {
        Err(AppError::InvalidArgument(format!(
            "{} is not a git repository",
            repo_root.display()
        )))
    }
}

fn ensure_remote_exists(repo_root: &Path, remote: &str) -> Result<(), AppError> {
    git_checked(repo_root, &["remote", "get-url", remote]).map(|_| ())
}

fn remote_ref_exists(repo_root: &Path, endpoint: &RefEndpoint) -> Result<bool, AppError> {
    let output = git_output(
        repo_root,
        &[
            "ls-remote",
            "--exit-code",
            &endpoint.remote,
            &endpoint.refname,
        ],
    )?;
    if output.status.success() {
        return Ok(true);
    }
    if output.status.code() == Some(2) {
        return Ok(false);
    }
    Err(command_failure(
        repo_root,
        &["ls-remote", &endpoint.remote],
        output,
    ))
}

fn git_checked(repo_root: &Path, args: &[&str]) -> Result<String, AppError> {
    let output = git_output(repo_root, args)?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(command_failure(repo_root, args, output))
    }
}

fn git_checked_bytes(repo_root: &Path, args: &[&str]) -> Result<Vec<u8>, AppError> {
    let output = git_output(repo_root, args)?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(command_failure(repo_root, args, output))
    }
}

fn git_output(repo_root: &Path, args: &[&str]) -> Result<Output, AppError> {
    Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .map_err(AppError::Io)
}

fn command_failure(repo_root: &Path, args: &[&str], output: Output) -> AppError {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    AppError::InvalidArgument(format!(
        "git command failed (code {:?}): git -C {} {} ({})",
        output.status.code(),
        repo_root.display(),
        args.join(" "),
        stderr
    ))
}

fn path_str(path: &Path) -> Result<&str, AppError> {
    path.to_str()
        .ok_or_else(|| AppError::InvalidArgument(format!("path is not UTF-8: {}", path.display())))
}

#[cfg(test)]
#[path = "sync_ref_migrate_tests.rs"]
mod tests;
