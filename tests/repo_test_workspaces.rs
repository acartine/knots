//! Guardrails that keep test workspaces on the shared RAII guard.
//!
//! Before knot 4701 this repository had 85 hand-rolled workspace helpers and
//! 634 best-effort `remove_dir_all` cleanup lines that only ran on the happy
//! path. Fixing that once is not enough — without these checks the pattern
//! simply grows back one test at a time.

use std::path::{Path, PathBuf};

/// Production code that legitimately builds its own temp directory. Each entry
/// is a real runtime workspace, not a test fixture, and is responsible for its
/// own cleanup; the reaper is the backstop when a process dies mid-run.
///
/// The test-support crate is a dev-dependency, so production code cannot reach
/// the guard. Adding a row here is a deliberate decision, not a formality.
const PRODUCTION_TEMP_DIR_USERS: &[&str] = &[
    "src/perf.rs",
    "src/loom_compat_harness.rs",
    "src/sync_ref_migrate/publish.rs",
];

/// Removals that are the behaviour under test or a product feature, not
/// workspace cleanup:
///
/// - `src/doctor_fix.rs` repairs a dirty worktree by removing it. That is what
///   the fix does.
/// - `src/loom_compat_harness_tests.rs` reclaims the workspace the *production*
///   compat harness kept via `keep_artifacts`, which no guard owns.
const DELIBERATE_REMOVALS: &[&str] = &["src/doctor_fix.rs", "src/loom_compat_harness_tests.rs"];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn rust_sources() -> Vec<PathBuf> {
    let mut sources = Vec::new();
    for dir in ["src", "tests"] {
        collect(&repo_root().join(dir), &mut sources);
    }
    // This file quotes every banned pattern in order to search for it.
    sources.retain(|path| !path.ends_with("repo_test_workspaces.rs"));
    sources.sort();
    sources
}

fn collect(dir: &Path, sources: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, sources);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            sources.push(path);
        }
    }
}

fn relative(path: &Path) -> String {
    path.strip_prefix(repo_root())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn tests_do_not_build_their_own_temp_directories() {
    let mut offenders = Vec::new();
    for path in rust_sources() {
        let name = relative(&path);
        if PRODUCTION_TEMP_DIR_USERS.contains(&name.as_str()) {
            continue;
        }
        let contents = std::fs::read_to_string(&path).expect("source should be readable");
        for (index, line) in contents.lines().enumerate() {
            // `starts_with(std::env::temp_dir())` is an assertion about where a
            // workspace landed, not a second way of creating one.
            if line.contains("env::temp_dir()") && !line.contains("starts_with") {
                offenders.push(format!("{name}:{}", index + 1));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "use knots_test_support::workspace(..) instead of building a temp path by hand: {offenders:#?}"
    );
}

#[test]
fn tests_do_not_clean_up_workspaces_by_hand() {
    let mut offenders = Vec::new();
    for path in rust_sources() {
        let name = relative(&path);
        if PRODUCTION_TEMP_DIR_USERS.contains(&name.as_str())
            || DELIBERATE_REMOVALS.contains(&name.as_str())
        {
            continue;
        }
        let contents = std::fs::read_to_string(&path).expect("source should be readable");
        for (index, line) in contents.lines().enumerate() {
            if line.trim().starts_with("let _ = std::fs::remove_dir_all(")
                || line.trim().starts_with("let _ = fs::remove_dir_all(")
            {
                offenders.push(format!("{name}:{}", index + 1));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "best-effort cleanup does not run when a test panics; hold a \
         knots_test_support::TestWorkspace guard instead: {offenders:#?}"
    );
}

#[test]
fn the_workspace_guard_is_never_dropped_on_the_line_that_creates_it() {
    // `let root = workspace("x").path().to_path_buf();` compiles, drops the
    // guard at the end of that statement, and deletes the directory before the
    // test body runs. It is the one way this pattern fails silently.
    let mut offenders = Vec::new();
    for path in rust_sources() {
        let name = relative(&path);
        let contents = std::fs::read_to_string(&path).expect("source should be readable");
        for (index, line) in contents.lines().enumerate() {
            let collapsed = line.contains("workspace(") && line.contains(".path()");
            if collapsed && !line.contains("_ws") {
                offenders.push(format!("{name}:{}", index + 1));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "bind the guard to a live local first: let ws = workspace(..); \
         let root = ws.path().to_path_buf(); — offenders: {offenders:#?}"
    );
}

#[test]
fn tests_do_not_hard_code_the_temp_root() {
    let mut offenders = Vec::new();
    for path in rust_sources() {
        let name = relative(&path);
        let contents = std::fs::read_to_string(&path).expect("source should be readable");
        for (index, line) in contents.lines().enumerate() {
            if line.contains("/tmp/") || line.contains("\"/tmp\"") {
                offenders.push(format!("{name}:{}", index + 1));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "a test must not name /tmp: use a workspace for real paths and a \
         neutral literal such as /example for fixtures: {offenders:#?}"
    );
}
