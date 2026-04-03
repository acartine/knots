use std::cell::Cell;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use crate::db::{self, KnotCacheRecord};
use crate::events::EventWriter;
use crate::installed_workflows;
use crate::locks::FileLock;
use crate::project::{DistributionMode, GlobalConfig, ProjectContext, StorePaths};
use crate::replication::ReplicationService;
use crate::sync::{SyncError, SyncSummary};
use crate::workflow::{ProfileDefinition, ProfileRegistry};

mod alias;
mod edges;
pub mod error;
mod gate;
mod gate_metadata;
pub mod helpers;
mod knot_create;
mod knot_lease;
mod knot_profile;
mod knot_update;
mod profile_config;
mod query;
pub mod rehydrate;
mod state_ops;
mod state_resolve;
mod sync_ops;
pub mod types;

pub use error::AppError;
pub use types::{
    CreateKnotOptions, EdgeView, GateDecision, KnotView, StateActorMetadata, UpdateKnotPatch,
};

#[cfg(test)]
pub(crate) use helpers::{
    ensure_profile_etag, metadata_entry_from_input, non_empty, normalize_tag, parse_edge_direction,
};
#[cfg(test)]
pub(crate) use rehydrate::apply_event::apply_rehydrate_event;
#[cfg(test)]
pub(crate) use rehydrate::{rehydrate_from_events, RehydrateProjection};
#[cfg(test)]
pub(crate) use types::ChildSummary;

pub type UserConfig = GlobalConfig;

const DEFAULT_PROFILE_ID: &str = "autopilot";

pub struct App {
    conn: Connection,
    writer: EventWriter,
    repo_root: PathBuf,
    store_paths: StorePaths,
    distribution: DistributionMode,
    project_id: Option<String>,
    profile_registry: ProfileRegistry,
    home_override: Option<Option<PathBuf>>,
    auto_sync_done: Cell<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncPolicy {
    Auto,
    Always,
    Never,
}

impl App {
    pub fn open(db_path: &str, repo_root: PathBuf) -> Result<Self, AppError> {
        let context = ProjectContext {
            project_id: None,
            repo_root: repo_root.clone(),
            store_paths: StorePaths {
                root: repo_root.join(".knots"),
            },
            distribution: DistributionMode::Git,
        };
        Self::open_with_context(&context, db_path)
    }

    pub fn open_with_context(context: &ProjectContext, db_path: &str) -> Result<Self, AppError> {
        let db = std::path::Path::new(db_path);
        let is_default = db
            .components()
            .next()
            .is_some_and(|c| c.as_os_str() == ".knots");
        if is_default
            && context.distribution == DistributionMode::Git
            && !context.store_paths.root.exists()
        {
            return Err(AppError::NotInitialized);
        }
        helpers::ensure_parent_dir(db_path)?;
        let conn = crate::trace::measure("db_open", || db::open_connection(db_path))?;
        let profile_registry = crate::trace::measure("profile_registry", || {
            ProfileRegistry::load_for_repo(&context.repo_root)
        })?;
        let writer = EventWriter::new(context.store_paths.root.clone());
        Ok(Self {
            conn,
            writer,
            repo_root: context.repo_root.clone(),
            store_paths: context.store_paths.clone(),
            distribution: context.distribution,
            project_id: context.project_id.clone(),
            profile_registry,
            home_override: None,
            auto_sync_done: Cell::new(false),
        })
    }

    pub(crate) fn with_home_override(mut self, home: Option<PathBuf>) -> Self {
        self.home_override = Some(home);
        self
    }

    fn repo_lock_path(&self) -> PathBuf {
        self.store_paths.repo_lock_path()
    }

    fn cache_lock_path(&self) -> PathBuf {
        self.store_paths.cache_lock_path()
    }

    fn read_sync_policy(&self) -> Result<SyncPolicy, AppError> {
        let raw = db::get_meta(&self.conn, "sync_policy")?.unwrap_or_else(|| "auto".to_string());
        match raw.trim().to_ascii_lowercase().as_str() {
            "always" => Ok(SyncPolicy::Always),
            "never" => Ok(SyncPolicy::Never),
            _ => Ok(SyncPolicy::Auto),
        }
    }

    fn read_sync_budget_ms(&self) -> Result<u64, AppError> {
        let raw =
            db::get_meta(&self.conn, "sync_auto_budget_ms")?.unwrap_or_else(|| "750".to_string());
        Ok(raw.trim().parse::<u64>().unwrap_or(750))
    }

    fn read_auto_sync_min_interval_ms(&self) -> Result<u64, AppError> {
        let raw = db::get_meta(&self.conn, "sync_auto_min_interval_ms")?
            .unwrap_or_else(|| "30000".to_string());
        Ok(raw.trim().parse::<u64>().unwrap_or(30_000))
    }

    fn read_pull_drift_warn_threshold(&self) -> Result<u64, AppError> {
        Ok(db::get_pull_drift_warn_threshold(&self.conn)?)
    }

    fn fallback_profile_id(&self) -> Result<String, AppError> {
        if self.profile_registry.require(DEFAULT_PROFILE_ID).is_ok() {
            return Ok(DEFAULT_PROFILE_ID.to_string());
        }
        self.profile_registry
            .list()
            .into_iter()
            .next()
            .map(|p| p.id)
            .ok_or_else(|| AppError::InvalidArgument("no profiles are defined".to_string()))
    }

    fn read_user_config(&self) -> Result<UserConfig, AppError> {
        match self.home_override.as_ref() {
            Some(Some(home)) => crate::project::read_global_config(Some(home.as_path()))
                .map_err(AppError::InvalidArgument),
            Some(None) => Ok(UserConfig::default()),
            None => crate::project::read_global_config(None).map_err(AppError::InvalidArgument),
        }
    }

    fn write_user_config(&self, config: &UserConfig) -> Result<(), AppError> {
        match self.home_override.as_ref() {
            Some(Some(home)) => crate::project::write_global_config(Some(home.as_path()), config)
                .map_err(AppError::InvalidArgument),
            Some(None) => Err(AppError::InvalidArgument(
                "unable to resolve $HOME for profile config".to_string(),
            )),
            None => {
                crate::project::write_global_config(None, config).map_err(AppError::InvalidArgument)
            }
        }
    }

    fn resolve_config_profile(&self, raw: &Option<String>) -> Option<String> {
        let raw_id = raw.as_deref()?;
        self.resolve_profile_id(raw_id, None).ok()
    }

    fn is_git_distribution(&self) -> bool {
        self.distribution == DistributionMode::Git
    }

    fn require_git_distribution(&self, action: &str) -> Result<(), AppError> {
        if self.is_git_distribution() {
            Ok(())
        } else {
            Err(AppError::UnsupportedDistribution {
                action: action.to_string(),
                mode: "local-only".to_string(),
            })
        }
    }

    fn current_workflow_id(&self) -> Result<String, AppError> {
        let registry = installed_workflows::InstalledWorkflowRegistry::load(&self.repo_root)?;
        Ok(registry.current_workflow_id().to_string())
    }

    pub fn default_workflow_id(&self) -> Result<String, AppError> {
        self.current_workflow_id()
    }

    fn mark_sync_pending(&self) -> Result<(), AppError> {
        db::set_meta(&self.conn, "sync_pending", "true")?;
        Ok(())
    }

    fn maybe_auto_sync_for_read(&self) -> Result<(), AppError> {
        if self.auto_sync_done.get() {
            crate::trace::record(
                "auto_sync",
                std::time::Duration::ZERO,
                Some("skipped:already_synced".to_string()),
            );
            return Ok(());
        }
        if !self.is_git_distribution() {
            crate::trace::record(
                "auto_sync",
                std::time::Duration::ZERO,
                Some("skipped:non_git".to_string()),
            );
            return Ok(());
        }
        let result = match self.read_sync_policy()? {
            SyncPolicy::Never => {
                crate::trace::record(
                    "auto_sync",
                    std::time::Duration::ZERO,
                    Some("skipped:policy=never".to_string()),
                );
                Ok(())
            }
            SyncPolicy::Always => {
                let _ = crate::trace::measure("auto_sync", || self.pull())?;
                Ok(())
            }
            SyncPolicy::Auto => self.try_auto_sync_for_read(),
        };
        if result.is_ok() {
            self.auto_sync_done.set(true);
        }
        result
    }

    fn try_auto_sync_for_read(&self) -> Result<(), AppError> {
        let min_interval_ms = self.read_auto_sync_min_interval_ms()?;
        if self.synced_recently(min_interval_ms)? {
            crate::trace::record(
                "auto_sync",
                std::time::Duration::ZERO,
                Some(format!("skipped:recent_sync<{}ms", min_interval_ms)),
            );
            return Ok(());
        }
        let mut repo_lock = crate::trace::phase("repo_lock");
        let rl = FileLock::try_acquire(&self.repo_lock_path())?;
        let Some(_rg) = rl else {
            repo_lock.detail("skipped:busy");
            return self.mark_sync_pending();
        };
        repo_lock.detail("acquired");
        let mut cache_lock = crate::trace::phase("cache_lock");
        let cl = FileLock::try_acquire(&self.cache_lock_path())?;
        let Some(_cg) = cl else {
            cache_lock.detail("skipped:busy");
            return self.mark_sync_pending();
        };
        cache_lock.detail("acquired");
        let start = Instant::now();
        let sync_result = crate::trace::measure("auto_sync", || self.pull_unlocked());
        if let Err(err) = sync_result {
            return match err {
                AppError::Sync(SyncError::GitCommandFailed { .. })
                | AppError::Sync(SyncError::GitUnavailable)
                | AppError::Sync(SyncError::ActiveLeasesExist(_)) => {
                    self.mark_sync_pending()?;
                    Ok(())
                }
                other => Err(other),
            };
        }
        let budget = self.read_sync_budget_ms()? as u128;
        if start.elapsed().as_millis() > budget {
            self.mark_sync_pending()?;
        }
        Ok(())
    }

    fn synced_recently(&self, min_interval_ms: u64) -> Result<bool, AppError> {
        let Some(raw) = db::get_meta(&self.conn, "last_sync_success_at_ms")? else {
            return Ok(false);
        };
        let Ok(last_sync) = raw.parse::<u128>() else {
            return Ok(false);
        };
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        Ok(now_ms.saturating_sub(last_sync) < u128::from(min_interval_ms))
    }

    fn pull_unlocked(&self) -> Result<SyncSummary, AppError> {
        self.require_git_distribution("pull")?;
        let svc = ReplicationService::with_store_paths(
            &self.conn,
            self.repo_root.clone(),
            self.store_paths.clone(),
        );
        Ok(svc.pull()?)
    }

    fn pull_unlocked_with_progress(
        &self,
        reporter: &mut Option<&mut dyn crate::progress::ProgressReporter>,
    ) -> Result<SyncSummary, AppError> {
        self.require_git_distribution("pull")?;
        let svc = ReplicationService::with_store_paths(
            &self.conn,
            self.repo_root.clone(),
            self.store_paths.clone(),
        );
        Ok(svc.pull_with_progress(reporter)?)
    }

    fn resolve_profile_for_record<'a>(
        &'a self,
        record: &KnotCacheRecord,
    ) -> Result<&'a ProfileDefinition, AppError> {
        let pid = helpers::non_empty(record.profile_id.as_str()).ok_or_else(|| {
            AppError::InvalidArgument(format!("knot '{}' is missing profile_id", record.id))
        })?;
        Ok(self.profile_registry.require(&pid)?)
    }
}

#[cfg(test)]
#[path = "app/tests.rs"]
mod tests;
#[cfg(test)]
#[path = "app/tests_acceptance_ext.rs"]
mod tests_acceptance_ext;
#[cfg(test)]
#[path = "app/tests_coverage_ext.rs"]
mod tests_coverage_ext;
#[cfg(test)]
#[path = "app/tests_coverage_ext2.rs"]
mod tests_coverage_ext2;
#[cfg(test)]
#[path = "app/tests_deferred_sync.rs"]
mod tests_deferred_sync;
#[cfg(test)]
#[path = "app/tests_error_paths.rs"]
mod tests_error_paths;
#[cfg(test)]
#[path = "app/tests_gate_ext.rs"]
mod tests_gate_ext;
#[cfg(test)]
#[path = "app/tests_hierarchy.rs"]
mod tests_hierarchy;
#[cfg(test)]
#[path = "app/tests_hierarchy_auto_resolve.rs"]
mod tests_hierarchy_auto_resolve;
#[cfg(test)]
#[path = "app/tests_hierarchy_ext.rs"]
mod tests_hierarchy_ext;
#[cfg(test)]
#[path = "app/tests_show_lease.rs"]
mod tests_show_lease;
#[cfg(test)]
#[path = "app/tests_step_history.rs"]
mod tests_step_history;
#[cfg(test)]
#[path = "app/tests_step_metadata_responses.rs"]
mod tests_step_metadata_responses;
#[cfg(test)]
#[path = "app/tests_terminal_deferred.rs"]
mod tests_terminal_deferred;
#[cfg(test)]
#[path = "app/tests_update_ext.rs"]
mod tests_update_ext;
