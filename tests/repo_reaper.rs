use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, SystemTime};

use knots_test_support::workspace;

/// Copy the reaper into a throwaway repository root inside `temp_root`.
///
/// The script derives its repository root from its own location, so running the
/// checked-in copy would point `$ROOT/target` at this repository's real build
/// directory. Copying it into the sandbox keeps both halves of the sweep —
/// build artifacts and temp workspaces — inside the test's own tree.
fn sandboxed_reaper(temp_root: &Path) -> PathBuf {
    let script_dir = temp_root.join("repo").join("scripts").join("repo");
    std::fs::create_dir_all(&script_dir).expect("sandbox script dir should be creatable");
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("repo")
        .join("reap-stale-artifacts.sh");
    let destination = script_dir.join("reap-stale-artifacts.sh");
    std::fs::copy(&source, &destination).expect("reaper script should be copyable");
    destination
}

/// Run the reaper with `temp_root` as its temp root so the test never touches
/// the ambient temp directory.
fn run_reaper(temp_root: &Path, max_age_hours: &str) -> Output {
    Command::new("bash")
        .arg(sandboxed_reaper(temp_root))
        .arg(max_age_hours)
        .env("TMPDIR", temp_root)
        .output()
        .expect("reaper script should run")
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Create `name` under `root` holding one file, then backdate both so the
/// reaper sees the tree as older than any positive cutoff.
fn aged_tree(root: &Path, name: &str) -> PathBuf {
    let tree = root.join(name);
    std::fs::create_dir_all(tree.join("nested")).expect("tree should be creatable");
    std::fs::write(tree.join("nested").join("file"), b"stale").expect("file should be written");

    let long_ago = SystemTime::now() - Duration::from_secs(72 * 3600);
    let stamp = filetime_from(long_ago);
    for path in [
        tree.join("nested").join("file"),
        tree.join("nested"),
        tree.clone(),
    ] {
        set_mtime(&path, &stamp);
    }
    tree
}

fn fresh_tree(root: &Path, name: &str) -> PathBuf {
    let tree = root.join(name);
    std::fs::create_dir_all(&tree).expect("tree should be creatable");
    std::fs::write(tree.join("file"), b"fresh").expect("file should be written");
    tree
}

fn filetime_from(time: SystemTime) -> String {
    let secs = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("timestamp should be after the unix epoch")
        .as_secs();
    format!("@{secs}")
}

fn set_mtime(path: &Path, stamp: &str) {
    let status = Command::new("touch")
        .arg("-h")
        .arg("-d")
        .arg(stamp)
        .arg(path)
        .status()
        .expect("touch should run");
    assert!(status.success(), "touch should succeed for {path:?}");
}

#[test]
fn reaps_stale_temp_workspaces_and_keeps_fresh_ones() {
    let ws = workspace("knots-reaper-test");
    let temp_root = ws.path().to_path_buf();

    let stale = aged_tree(&temp_root, "knots-stale-workspace");
    let fresh = fresh_tree(&temp_root, "knots-fresh-workspace");

    let output = run_reaper(&temp_root, "24");

    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    assert!(!stale.exists(), "a stale knots-* workspace should be reaped");
    assert!(fresh.exists(), "a fresh knots-* workspace should survive");
    assert!(
        stdout_of(&output).contains("Reaping stale artifact tree:"),
        "stdout: {}",
        stdout_of(&output)
    );
}

#[test]
fn still_reaps_stale_build_artifacts_under_target() {
    let ws = workspace("knots-reaper-test");
    let temp_root = ws.path().to_path_buf();
    sandboxed_reaper(&temp_root);

    let target = temp_root.join("repo").join("target");
    std::fs::create_dir_all(&target).expect("target dir should be creatable");
    let stale = aged_tree(&target, "debug");
    let fresh = fresh_tree(&target, "release");

    let output = run_reaper(&temp_root, "24");

    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    assert!(!stale.exists(), "a stale target subtree should still be reaped");
    assert!(fresh.exists(), "a fresh target subtree should survive");
    assert!(
        stdout_of(&output).contains("Reaping stale artifact tree: target/debug"),
        "the target sweep should report repo-relative paths, stdout: {}",
        stdout_of(&output)
    );
}

#[test]
fn leaves_unrelated_temp_directories_alone() {
    let ws = workspace("knots-reaper-test");
    let temp_root = ws.path().to_path_buf();

    let unrelated = aged_tree(&temp_root, "cargo-install-something");

    let output = run_reaper(&temp_root, "24");

    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    assert!(
        unrelated.exists(),
        "a directory that is not named knots-* must not be reaped"
    );
}

#[test]
fn reports_when_nothing_was_removed() {
    let ws = workspace("knots-reaper-test");
    let temp_root = ws.path().to_path_buf();

    fresh_tree(&temp_root, "knots-fresh-workspace");

    let output = run_reaper(&temp_root, "24");

    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    assert!(
        stdout_of(&output).contains("No stale build artifacts older than 24h."),
        "stdout: {}",
        stdout_of(&output)
    );
}

#[test]
fn refuses_a_zero_cutoff() {
    let ws = workspace("knots-reaper-test");
    let output = run_reaper(ws.path(), "0");

    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr_of(&output).contains("MAX_AGE_HOURS must be greater than zero"),
        "stderr: {}",
        stderr_of(&output)
    );
}

#[test]
fn refuses_a_non_numeric_cutoff() {
    let ws = workspace("knots-reaper-test");
    let output = run_reaper(ws.path(), "abc");

    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr_of(&output).contains("MAX_AGE_HOURS must be a positive integer"),
        "stderr: {}",
        stderr_of(&output)
    );
}

#[test]
fn does_not_reap_its_own_cutoff_stamp_file() {
    let ws = workspace("knots-reaper-test");
    let temp_root = ws.path().to_path_buf();

    // A stamp file left behind by a previous run matches the `knots-*` glob but
    // is a file, so the directory-only sweep must ignore it rather than report
    // it as a reaped tree.
    let leftover = temp_root.join("knots-artifact-cutoff.ABCDEF");
    std::fs::write(&leftover, b"").expect("stamp file should be writable");
    set_mtime(
        &leftover,
        &filetime_from(SystemTime::now() - Duration::from_secs(72 * 3600)),
    );

    let output = run_reaper(&temp_root, "24");

    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    assert!(leftover.exists(), "a stale stamp file is not an artifact tree");
    assert!(
        stdout_of(&output).contains("No stale build artifacts older than 24h."),
        "stdout: {}",
        stdout_of(&output)
    );
}
