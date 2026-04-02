#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use rusqlite::params;
use uuid::Uuid;

pub fn unique_workspace(prefix: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&path).expect("workspace should be creatable");
    path
}

pub fn run_git(cwd: &Path, args: &[&str]) {
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

pub fn setup_repo(root: &Path) {
    run_git(root, &["init"]);
    run_git(root, &["config", "user.email", "knots@example.com"]);
    run_git(root, &["config", "user.name", "Knots Test"]);
    std::fs::write(root.join("README.md"), "# knots\n").expect("readme should be writable");
    run_git(root, &["add", "README.md"]);
    run_git(root, &["commit", "-m", "init"]);
    run_git(root, &["branch", "-M", "main"]);
}

pub fn setup_repo_with_remote(root: &Path) -> PathBuf {
    setup_repo(root);
    let remote = root.join("remote.git");
    run_git(
        root,
        &["init", "--bare", remote.to_str().expect("utf8 path")],
    );
    run_git(
        root,
        &[
            "remote",
            "add",
            "origin",
            remote.to_str().expect("utf8 path"),
        ],
    );
    run_git(root, &["push", "-u", "origin", "main"]);
    let output = Command::new("git")
        .arg("--git-dir")
        .arg(&remote)
        .args(["symbolic-ref", "HEAD", "refs/heads/main"])
        .output()
        .expect("git symbolic-ref should run");
    assert!(
        output.status.success(),
        "git symbolic-ref failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    remote
}

pub fn knots_binary() -> PathBuf {
    let configured = PathBuf::from(env!("CARGO_BIN_EXE_knots"));
    if configured.is_absolute() && configured.exists() {
        return configured;
    }
    if configured.exists() {
        return std::fs::canonicalize(&configured).unwrap_or(configured);
    }
    let manifest_relative = Path::new(env!("CARGO_MANIFEST_DIR")).join(&configured);
    if manifest_relative.exists() {
        return std::fs::canonicalize(&manifest_relative).unwrap_or(manifest_relative);
    }

    if let Ok(current_exe) = std::env::current_exe() {
        if !configured.is_absolute() {
            for ancestor in current_exe.ancestors().skip(1) {
                let candidate = ancestor.join(&configured);
                if candidate.exists() {
                    return std::fs::canonicalize(&candidate).unwrap_or(candidate);
                }
            }
        }

        if let Some(debug_dir) = current_exe.parent().and_then(|deps| deps.parent()) {
            for name in ["knots", "knots.exe"] {
                let candidate = debug_dir.join(name);
                if candidate.exists() {
                    return std::fs::canonicalize(&candidate).unwrap_or(candidate);
                }
            }
        }
    }

    configured
}

fn configure_coverage_env(command: &mut Command) {
    let _ = command;
}

pub fn run_knots(repo_root: &Path, db_path: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(knots_binary());
    command
        .arg("--repo-root")
        .arg(repo_root)
        .arg("--db")
        .arg(db_path)
        .env("KNOTS_SKIP_DOCTOR_UPGRADE", "1")
        .args(args);
    configure_coverage_env(&mut command);
    command.output().expect("knots command should run")
}

pub fn set_meta_value(db_path: &Path, key: &str, value: &str) {
    let conn = rusqlite::Connection::open(db_path).expect("db should open");
    conn.execute(
        r#"
INSERT INTO meta (key, value)
VALUES (?1, ?2)
ON CONFLICT(key) DO UPDATE SET value = excluded.value
"#,
        params![key, value],
    )
    .expect("meta should be writable");
}

pub fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "expected success but failed.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub fn assert_failure(output: &Output) {
    assert!(
        !output.status.success(),
        "expected failure but succeeded.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub fn parse_created_id(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .split_whitespace()
        .nth(1)
        .expect("created output should include knot id")
        .to_string()
}

pub fn assert_contains_in_order(haystack: &str, needles: &[&str]) {
    let mut offset = 0usize;
    for needle in needles {
        let found = haystack[offset..]
            .find(needle)
            .unwrap_or_else(|| panic!("missing '{needle}' in output:\n{haystack}"));
        offset += found + needle.len();
    }
}

pub fn prompt_excerpt(prompt: &str) -> &str {
    &prompt[..prompt.len().min(500)]
}

pub fn assert_prompt_contains(prompt: &str, needle: &str, context: &str) {
    assert!(
        prompt.contains(needle),
        "{context}: expected rendered prompt to contain {needle:?}.\n\
         Prompt excerpt:\n{}",
        prompt_excerpt(prompt)
    );
}

pub fn assert_prompt_not_contains(prompt: &str, needle: &str, context: &str) {
    assert!(
        !prompt.contains(needle),
        "{context}: unexpected rendered prompt content matched {needle:?}.\n\
         Prompt excerpt:\n{}",
        prompt_excerpt(prompt)
    );
}
