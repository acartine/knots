//! File discovery for `IncrementalApplier`, plus draining the per-knot
//! quarantine table.
//!
//! A pull's shared watermark (`last_index_head_commit` /
//! `last_full_head_commit`) always advances, even in a pass that skips some
//! knots' events because this machine holds a local lease on them. Losing
//! track of those events would be strictly worse than deferring, so each
//! skipped knot gets its own quarantine row anchored to the watermark from
//! *before* the pass that first skipped it. `drain_quarantine` is how a
//! later pull catches a knot back up once it is no longer locally leased.

use std::collections::HashSet;
use std::path::PathBuf;

use crate::db;

use super::{IncrementalApplier, SyncError};

/// Git's well-known empty-tree object id, present in every repository. Used
/// as the quarantine base when a knot is first skipped before this machine
/// has ever pulled, so "diff from nothing" behaves like a full scan.
const EMPTY_TREE_SHA: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

impl<'a> IncrementalApplier<'a> {
    /// The watermark to anchor any newly quarantined knot to for this pass.
    pub(super) fn resolve_pre_pass_base_commit(&self) -> Result<String, SyncError> {
        Ok(db::get_meta(self.conn, "last_index_head_commit")?
            .unwrap_or_else(|| EMPTY_TREE_SHA.to_string()))
    }

    pub(super) fn changed_files(
        &self,
        meta_key: &str,
        prefix: &str,
        target_head: &str,
    ) -> Result<Vec<PathBuf>, SyncError> {
        let base = db::get_meta(self.conn, meta_key)?;
        self.files_since(base.as_deref(), prefix, target_head)
    }

    fn files_since(
        &self,
        base: Option<&str>,
        prefix: &str,
        target_head: &str,
    ) -> Result<Vec<PathBuf>, SyncError> {
        if let Some(base_head) = base {
            if base_head == target_head {
                return Ok(Vec::new());
            }

            match self
                .git
                .diff_name_only(&self.worktree, base_head, target_head, prefix)
            {
                Ok(mut files) => {
                    files.retain(|path| path.extension().is_some_and(|ext| ext == "json"));
                    files.sort();
                    return Ok(files);
                }
                Err(err) if err.is_unknown_revision() => {}
                Err(err) => return Err(err),
            }
        }

        let mut files = self.scan_json_files(prefix)?;
        files.sort();
        Ok(files)
    }

    fn scan_json_files(&self, prefix: &str) -> Result<Vec<PathBuf>, SyncError> {
        let root = self.worktree.join(prefix);
        if !root.exists() {
            return Ok(Vec::new());
        }

        let mut stack = vec![root];
        let mut files = Vec::new();
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
                    .strip_prefix(&self.worktree)
                    .map_err(|err| SyncError::InvalidEvent {
                        path: path.clone(),
                        message: format!("failed to relativize path: {}", err),
                    })?
                    .to_path_buf();
                files.push(relative);
            }
        }
        Ok(files)
    }

    /// Replay events for knots whose quarantine can now drain: skipped in an
    /// earlier pull because this machine held a local lease on them, and no
    /// longer locally leased now. Each knot's own `base_commit` -- captured
    /// the first time it was skipped -- anchors a diff up to `target_head`,
    /// independent of the shared watermark. A knot still locally leased is
    /// left alone; its row (and base commit) survive untouched.
    ///
    /// `apply_index_event` / `apply_full_event` re-check the locally-leased
    /// set themselves, so applying every file this scan turns up (not just
    /// ones belonging to the draining knot) is safe: anything for a knot
    /// that is still locally leased is quarantined again as a no-op.
    ///
    /// Returns the relative paths applied here, so the caller can skip them
    /// in the normal watermark-driven pass and avoid re-applying the same
    /// event twice in one call to `apply_to_head`.
    pub(super) fn drain_quarantine(
        &mut self,
        target_head: &str,
    ) -> Result<HashSet<PathBuf>, SyncError> {
        let mut applied = HashSet::new();
        for row in db::list_quarantined_knots(self.conn)? {
            if self.locally_leased_knot_ids.contains(&row.knot_id) {
                continue;
            }

            let index_files =
                self.files_since(Some(&row.base_commit), ".knots/index", target_head)?;
            for path in index_files {
                self.apply_index_event(&path)?;
                applied.insert(path);
            }

            let full_files =
                self.files_since(Some(&row.base_commit), ".knots/events", target_head)?;
            for path in full_files {
                self.apply_full_event(&path)?;
                applied.insert(path);
            }

            db::remove_quarantine(self.conn, &row.knot_id)?;
        }
        Ok(applied)
    }
}
