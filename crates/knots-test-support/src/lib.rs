//! Shared test-workspace support for the `knots` crate.
//!
//! `knots` is a binary-only crate, so integration tests under `tests/` cannot
//! import helpers from `src/`. This crate is the one place both sides can
//! reach, which is why the workspace helper lives here rather than in `src/`.
//!
//! Every test workspace is an RAII guard: the directory is removed when the
//! guard drops, so cleanup survives a panic in the middle of a test body.
//! Cleanup cannot survive `SIGKILL` or `abort`; the reaper in
//! `scripts/repo/reap-stale-artifacts.sh` is the backstop for that residue.

use std::path::{Path, PathBuf};

use uuid::Uuid;

/// A uniquely named directory under the temp root that removes itself on drop.
///
/// Hold the guard in a live binding for as long as the directory is needed:
///
/// ```ignore
/// let ws = knots_test_support::workspace("knots-example-test");
/// let root = ws.path().to_path_buf();
/// // ... `ws` stays alive until the end of the test ...
/// ```
///
/// Never collapse those two lines. `workspace("x").path().to_path_buf()` drops
/// the guard at the end of that statement and deletes the directory before the
/// test body runs.
#[derive(Debug)]
pub struct TestWorkspace {
    root: PathBuf,
}

impl TestWorkspace {
    /// Create and return a new workspace named `{prefix}-{uuid}` under the
    /// temp root.
    ///
    /// The temp root comes from [`std::env::temp_dir`], so `TMPDIR` is
    /// honoured and no `/tmp` literal ever appears in a test.
    pub fn new(prefix: &str) -> Self {
        let root = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&root).expect("workspace should be creatable");
        Self { root }
    }

    /// The workspace directory.
    pub fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl AsRef<Path> for TestWorkspace {
    fn as_ref(&self) -> &Path {
        self.path()
    }
}

/// Create a workspace under the temp root. See [`TestWorkspace`].
pub fn workspace(prefix: &str) -> TestWorkspace {
    TestWorkspace::new(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_a_directory_under_the_temp_root() {
        let ws = workspace("knots-test-support-create");
        assert!(ws.path().is_dir());
        assert!(ws.path().starts_with(std::env::temp_dir()));
        let name = ws
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .expect("workspace name should be valid utf-8");
        assert!(name.starts_with("knots-test-support-create-"));
    }

    #[test]
    fn distinct_workspaces_do_not_collide() {
        let first = workspace("knots-test-support-unique");
        let second = workspace("knots-test-support-unique");
        assert_ne!(first.path(), second.path());
    }

    #[test]
    fn drop_removes_the_tree_including_contents() {
        let path = {
            let ws = workspace("knots-test-support-drop");
            std::fs::create_dir_all(ws.path().join("nested"))
                .expect("nested dir should be created");
            std::fs::write(ws.path().join("nested").join("file"), b"contents")
                .expect("file should be written");
            ws.path().to_path_buf()
        };
        assert!(!path.exists(), "workspace should be removed on drop");
    }

    #[test]
    fn drop_runs_when_the_test_body_panics() {
        let recorded = std::sync::Arc::new(std::sync::Mutex::new(None::<PathBuf>));
        let sink = std::sync::Arc::clone(&recorded);

        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let outcome = std::panic::catch_unwind(move || {
            let ws = workspace("knots-test-support-panic");
            *sink.lock().expect("sink should not be poisoned") = Some(ws.path().to_path_buf());
            panic!("test body fails after creating its workspace");
        });
        std::panic::set_hook(previous);

        assert!(outcome.is_err(), "the closure should have panicked");
        let path = recorded
            .lock()
            .expect("sink should not be poisoned")
            .clone()
            .expect("workspace path should have been recorded");
        assert!(
            !path.exists(),
            "workspace should be removed despite the panic"
        );
    }

    #[test]
    fn as_ref_exposes_the_workspace_path() {
        let ws = workspace("knots-test-support-as-ref");
        let borrowed: &Path = ws.as_ref();
        assert_eq!(borrowed, ws.path());
    }
}
