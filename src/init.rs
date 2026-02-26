use std::io::{self, Write};
use std::path::Path;

use crate::app::AppError;
use crate::db;
use crate::remote_init::{
    detect_beads_hooks, init_remote_knots_branch, uninit_remote_knots_branch, RemoteInitError,
};

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_BOLD_CYAN: &str = "\x1b[1;36m";
const ANSI_BOLD_GREEN: &str = "\x1b[1;32m";
const ANSI_BOLD_MAGENTA: &str = "\x1b[1;35m";
const ANSI_BOLD_YELLOW: &str = "\x1b[1;33m";
const ANSI_DIM: &str = "\x1b[2m";

const KNOTS_IGNORE_RULE: &str = "/.knots/";

pub(crate) fn init_all(repo_root: &Path, db_path: &str) -> Result<(), AppError> {
    print_banner("FIT TO BE TIED 🎉")?;
    progress("initializing local store")?;
    init_local_store(repo_root, db_path)?;
    progress_ok("local store initialized")?;
    warn_if_beads_hooks_present(repo_root)?;
    progress("initializing remote branch origin/knots")?;
    progress_note("this can take a bit...")?;
    init_remote_knots_branch(repo_root)?;
    progress_ok("remote branch origin/knots initialized")?;
    Ok(())
}

pub(crate) fn uninit_all(repo_root: &Path, db_path: &str) -> Result<(), AppError> {
    print_banner("UNTYING THE KNOT 🎉")?;
    progress("removing local store")?;
    uninit_local_store(repo_root, db_path)?;
    progress_ok("local store removed")?;
    progress("removing remote branch origin/knots")?;
    progress_note("this can take a bit...")?;
    match uninit_remote_knots_branch(repo_root, "origin", "knots") {
        Ok(true) => progress_ok("remote branch origin/knots removed")?,
        Ok(false) => progress_warn("remote branch origin/knots not present")?,
        Err(RemoteInitError::NotGitRepository) => {
            progress_warn("not a git repository; skipping remote branch cleanup")?;
        }
        Err(RemoteInitError::MissingRemote(_)) => {
            progress_warn("origin remote is not configured; skipping remote branch cleanup")?;
        }
        Err(err) => return Err(err.into()),
    }
    Ok(())
}

pub(crate) fn init_local_store(repo_root: &Path, db_path: &str) -> Result<(), AppError> {
    if let Some(parent) = Path::new(db_path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    progress(&format!("opening cache database at {db_path}"))?;
    let _ = db::open_connection(db_path)?;
    progress("ensuring gitignore includes .knots rule")?;
    ensure_knots_gitignore(repo_root)?;
    progress_ok("local store ready")?;
    Ok(())
}

pub(crate) fn uninit_local_store(repo_root: &Path, db_path: &str) -> Result<(), AppError> {
    remove_gitignore_entries(repo_root)?;
    remove_db_file(db_path)?;
    let knots_dir = repo_root.join(".knots");
    if knots_dir.exists() {
        std::fs::remove_dir_all(&knots_dir)?;
    }
    progress_ok("local store removed")?;
    Ok(())
}

fn progress(message: &str) -> Result<(), AppError> {
    println!("{ANSI_BOLD_CYAN}•{ANSI_RESET} {message}");
    io::stdout().flush()?;
    Ok(())
}

fn progress_ok(message: &str) -> Result<(), AppError> {
    println!("{ANSI_BOLD_GREEN}✓{ANSI_RESET} {message}");
    io::stdout().flush()?;
    Ok(())
}

fn progress_warn(message: &str) -> Result<(), AppError> {
    println!("{ANSI_BOLD_YELLOW}!{ANSI_RESET} {message}");
    io::stdout().flush()?;
    Ok(())
}

fn progress_note(message: &str) -> Result<(), AppError> {
    println!("{ANSI_DIM}{message}{ANSI_RESET}");
    io::stdout().flush()?;
    Ok(())
}

fn print_banner(title: &str) -> Result<(), AppError> {
    println!("{ANSI_BOLD_MAGENTA}{title}{ANSI_RESET}");
    println!("{ANSI_BOLD_CYAN}Welcome to Knots!{ANSI_RESET}");
    println!(
        "{ANSI_DIM}version {}{ANSI_RESET}",
        env!("CARGO_PKG_VERSION")
    );
    println!();
    io::stdout().flush()?;
    Ok(())
}

fn warn_if_beads_hooks_present(repo_root: &Path) -> Result<(), AppError> {
    let report = detect_beads_hooks(repo_root);
    if report.is_empty() {
        return Ok(());
    }

    progress("found bd/beads hook-related setup in this repository")?;
    for hook in &report.hook_files {
        progress(&format!("  - hook: {}", hook.display()))?;
    }
    if report.has_beads_config {
        progress("  - git config section: [beads]")?;
    }

    progress("to disable bd/beads hooks and stop these push checks:")?;
    if !report.hook_files.is_empty() {
        for hook in &report.hook_files {
            progress(&format!("  rm {}", hook.display()))?;
        }
    } else {
        progress("  (no hook files matched; likely hooks are configured elsewhere)")?;
    }
    if report.has_beads_config {
        progress("  git config --remove-section beads")?;
    }
    Ok(())
}

pub(crate) fn ensure_knots_gitignore(repo_root: &Path) -> Result<(), AppError> {
    let path = repo_root.join(".gitignore");
    let contents = if path.exists() {
        std::fs::read_to_string(&path)?
    } else {
        String::new()
    };

    let has_ignore = contains_knots_ignore(&contents);
    if has_ignore {
        return Ok(());
    }

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;

    if !contents.is_empty() && !contents.ends_with('\n') {
        writeln!(file)?;
    }
    if !has_ignore {
        writeln!(file, "{}", KNOTS_IGNORE_RULE)?;
    }
    Ok(())
}

fn remove_gitignore_entries(repo_root: &Path) -> Result<(), AppError> {
    let path = repo_root.join(".gitignore");
    if !path.exists() {
        return Ok(());
    }

    let contents = std::fs::read_to_string(&path)?;
    let filtered: Vec<&str> = contents
        .lines()
        .map(str::trim)
        .filter(|line| {
            let line = *line;
            !(line == KNOTS_IGNORE_RULE || line.is_empty())
        })
        .collect();

    if filtered.len() == contents.lines().count() {
        return Ok(());
    }

    let new_contents = format!("{}\n", filtered.join("\n"));
    std::fs::write(path, new_contents)?;
    Ok(())
}

fn remove_db_file(db_path: &str) -> Result<(), AppError> {
    let path = Path::new(db_path);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

fn contains_knots_ignore(contents: &str) -> bool {
    contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .any(|line| {
            matches!(
                line,
                "/.knots" | "/.knots/" | "/.knots/*" | ".knots" | ".knots/" | ".knots/*"
            )
        })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::Command;
    use uuid::Uuid;

    use super::{init_all, init_local_store, uninit_all, uninit_local_store, KNOTS_IGNORE_RULE};

    fn unique_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("knots-init-test-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temp directory should be creatable");
        dir
    }

    fn remove_dir_if_exists(root: &PathBuf) {
        if root.exists() {
            let _ = std::fs::remove_dir_all(root);
        }
    }

    fn run_git(cwd: &PathBuf, args: &[&str]) {
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
        let root = unique_dir();
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
        std::fs::write(local.join("README.md"), "# knots\n").expect("readme should be writable");
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
    fn init_local_store_writes_expected_artifacts() {
        let root = unique_dir();
        let db_path = root.join(".knots/cache/state.sqlite");

        init_local_store(&root, db_path.to_str().expect("utf8 path"))
            .expect("local init should work");

        assert!(db_path.exists());

        let gitignore =
            std::fs::read_to_string(root.join(".gitignore")).expect("gitignore should be readable");
        assert!(gitignore.lines().any(|line| line == KNOTS_IGNORE_RULE));
        remove_dir_if_exists(&root);
    }

    #[test]
    fn init_local_store_does_not_create_repo_workflow_file() {
        let root = unique_dir();
        let db_path = root.join(".knots/cache/state.sqlite");

        init_local_store(&root, db_path.to_str().expect("utf8 path"))
            .expect("local init should succeed");

        assert!(!root.join(".knots/workflows.toml").exists());
        remove_dir_if_exists(&root);
    }

    #[test]
    fn init_local_store_is_idempotent_with_gitignore() {
        let root = unique_dir();
        let db_path = root.join(".knots/cache/state.sqlite");

        init_local_store(&root, db_path.to_str().expect("utf8 path"))
            .expect("first init should work");
        init_local_store(&root, db_path.to_str().expect("utf8 path"))
            .expect("second init should remain idempotent");

        let gitignore =
            std::fs::read_to_string(root.join(".gitignore")).expect("gitignore should be readable");
        let ignore_count = gitignore
            .lines()
            .filter(|line| *line == KNOTS_IGNORE_RULE)
            .count();
        assert_eq!(ignore_count, 1);
        remove_dir_if_exists(&root);
    }

    #[test]
    fn init_all_bootstraps_local_store_and_remote_branch() {
        let (root, local) = setup_repo_with_remote();
        let db_path = local.join(".knots/cache/state.sqlite");

        init_all(&local, db_path.to_str().expect("utf8 path")).expect("init should succeed");

        let output = Command::new("git")
            .arg("-C")
            .arg(&local)
            .args(["ls-remote", "--heads", "origin", "knots"])
            .output()
            .expect("git ls-remote should run");
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("refs/heads/knots"));

        let gitignore = std::fs::read_to_string(local.join(".gitignore"))
            .expect("gitignore should be readable");
        assert!(gitignore.lines().any(|line| line == KNOTS_IGNORE_RULE));
        remove_dir_if_exists(&root);
    }

    #[test]
    fn uninit_local_store_cleans_local_artifacts_and_gitignore() {
        let root = unique_dir();
        let db_path = root.join(".knots/cache/state.sqlite");
        let gitignore_path = root.join(".gitignore");

        init_local_store(&root, db_path.to_str().expect("utf8 path"))
            .expect("local init should succeed");
        assert!(root.join(".knots").exists());
        assert!(db_path.exists());

        uninit_local_store(&root, db_path.to_str().expect("utf8 path"))
            .expect("local uninit should succeed");

        assert!(!root.join(".knots").exists());
        assert!(!db_path.exists());
        if gitignore_path.exists() {
            let gitignore =
                std::fs::read_to_string(&gitignore_path).expect("gitignore should be readable");
            assert!(!gitignore.lines().any(|line| line == KNOTS_IGNORE_RULE));
        }
        remove_dir_if_exists(&root);
    }

    #[test]
    fn uninit_all_removes_remote_and_local_store() {
        let (root, local) = setup_repo_with_remote();
        let db_path = local.join(".knots/cache/state.sqlite");

        init_all(&local, db_path.to_str().expect("utf8 path")).expect("init should succeed");
        uninit_all(&local, db_path.to_str().expect("utf8 path")).expect("uninit should succeed");

        assert!(!local.join(".knots").exists());
        let output = Command::new("git")
            .arg("-C")
            .arg(&local)
            .args(["ls-remote", "--heads", "origin", "knots"])
            .output()
            .expect("git ls-remote should run");
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(!stdout.contains("refs/heads/knots"));
        remove_dir_if_exists(&root);
    }
}

#[cfg(test)]
#[path = "init_tests_ext.rs"]
mod tests_ext;
