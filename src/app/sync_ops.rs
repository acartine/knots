use std::time::Duration;

use crate::db;
use crate::doctor::{run_doctor_with_fix_at, DoctorReport};
use crate::fsck::{run_fsck_at_store, FsckReport};
use crate::locks::FileLock;
use crate::perf::{run_perf_harness, PerfReport};
use crate::progress::ProgressReporter;
use crate::remote_init::init_remote_knots_branch;
use crate::replication::{PushSummary, ReplicationService, ReplicationSummary, SyncOutcome};
use crate::snapshots::{write_snapshots_at_store, SnapshotWriteSummary};
use crate::sync::SyncSummary;

use super::error::AppError;
use super::types::PullDriftWarning;
use super::App;

impl App {
    pub fn pull(&self) -> Result<SyncSummary, AppError> {
        self.pull_with_progress(None)
    }

    pub fn pull_with_progress(
        &self,
        reporter: Option<&mut dyn ProgressReporter>,
    ) -> Result<SyncSummary, AppError> {
        self.require_git_distribution("pull")?;
        let mut reporter = reporter;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        self.pull_unlocked_with_progress(&mut reporter)
    }

    pub fn pull_drift_warning(&self) -> Result<Option<PullDriftWarning>, AppError> {
        self.require_git_distribution("pull")?;
        let threshold = self.read_pull_drift_warn_threshold()?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let service = ReplicationService::with_store_paths(
            &self.conn,
            self.repo_root.clone(),
            self.store_paths.clone(),
        );
        let unpushed = service.count_unpushed_event_files()?;
        if unpushed > threshold {
            Ok(Some(PullDriftWarning {
                unpushed_event_files: unpushed,
                threshold,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn push(&self) -> Result<PushSummary, AppError> {
        self.require_git_distribution("push")?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let service = ReplicationService::with_store_paths(
            &self.conn,
            self.repo_root.clone(),
            self.store_paths.clone(),
        );
        Ok(service.push()?)
    }

    pub fn push_with_progress(
        &self,
        reporter: Option<&mut dyn ProgressReporter>,
    ) -> Result<PushSummary, AppError> {
        self.require_git_distribution("push")?;
        let mut reporter = reporter;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let service = ReplicationService::with_store_paths(
            &self.conn,
            self.repo_root.clone(),
            self.store_paths.clone(),
        );
        Ok(service.push_with_progress(&mut reporter)?)
    }

    pub fn sync(&self) -> Result<ReplicationSummary, AppError> {
        self.require_git_distribution("sync")?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        let service = ReplicationService::with_store_paths(
            &self.conn,
            self.repo_root.clone(),
            self.store_paths.clone(),
        );
        Ok(service.sync()?)
    }

    #[allow(dead_code)]
    pub fn sync_with_progress(
        &self,
        reporter: Option<&mut dyn ProgressReporter>,
    ) -> Result<ReplicationSummary, AppError> {
        self.require_git_distribution("sync")?;
        let mut reporter = reporter;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        let service = ReplicationService::with_store_paths(
            &self.conn,
            self.repo_root.clone(),
            self.store_paths.clone(),
        );
        Ok(service.sync_with_progress(&mut reporter)?)
    }

    pub fn sync_or_defer_with_progress(
        &self,
        reporter: Option<&mut dyn ProgressReporter>,
    ) -> Result<SyncOutcome, AppError> {
        self.require_git_distribution("sync")?;
        let mut reporter = reporter;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        let service = ReplicationService::with_store_paths(
            &self.conn,
            self.repo_root.clone(),
            self.store_paths.clone(),
        );
        let outcome = service.sync_or_defer_with_progress(&mut reporter)?;
        if matches!(outcome, SyncOutcome::Deferred { .. }) {
            self.mark_sync_pending()?;
        }
        Ok(outcome)
    }

    pub fn trigger_queued_sync(&self) -> Result<bool, AppError> {
        let pending = db::get_meta(&self.conn, "sync_pending")?;
        if pending.as_deref() != Some("true") {
            return Ok(false);
        }
        if db::count_active_leases(&self.conn)? > 0 {
            return Ok(false);
        }
        match self.sync() {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    pub fn init_remote(&self) -> Result<(), AppError> {
        self.require_git_distribution("init-remote")?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        crate::init::ensure_knots_gitignore(&self.repo_root)?;
        init_remote_knots_branch(&self.repo_root)?;
        Ok(())
    }

    pub fn fsck(&self) -> Result<FsckReport, AppError> {
        Ok(run_fsck_at_store(&self.store_paths.root)?)
    }

    pub fn doctor(&self, fix: bool) -> Result<DoctorReport, AppError> {
        Ok(run_doctor_with_fix_at(
            &self.repo_root,
            &self.store_paths.root,
            self.distribution,
            fix,
        )?)
    }

    pub fn compact_write_snapshots(&self) -> Result<SnapshotWriteSummary, AppError> {
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        Ok(write_snapshots_at_store(
            &self.conn,
            &self.store_paths.root,
        )?)
    }

    pub fn perf_harness(&self, iterations: u32) -> Result<PerfReport, AppError> {
        let _ = self;
        Ok(run_perf_harness(iterations)?)
    }
}
