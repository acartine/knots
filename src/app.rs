use std::collections::{BTreeMap, HashSet};
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde::Serialize;
use serde_json::{json, Value};

use crate::db::{self, EdgeDirection, EdgeRecord, KnotCacheRecord, UpsertKnotHot};
use crate::doctor::{run_doctor_with_fix, DoctorError, DoctorReport};
use crate::domain::gate::{GateData, GateOwnerKind};
use crate::domain::invariant::Invariant;
use crate::domain::knot_type::{parse_knot_type, KnotType};
use crate::domain::lease::LeaseData;
use crate::domain::metadata::{normalize_datetime, MetadataEntry, MetadataEntryInput};
use crate::domain::state::{InvalidStateTransition, ParseKnotStateError};
use crate::domain::step_history::{derive_phase, StepActorInfo, StepRecord, StepStatus};
use crate::events::{
    new_event_id, now_utc_rfc3339, EventRecord, EventWriteError, EventWriter, FullEvent,
    FullEventKind, IndexEvent, IndexEventKind,
};
use crate::fsck::{run_fsck, FsckError, FsckReport};
use crate::hierarchy_alias::{build_alias_maps, AliasMaps};
use crate::knot_id::generate_knot_id;
use crate::locks::{FileLock, LockError};
use crate::perf::{run_perf_harness, PerfError, PerfReport};
use crate::progress::ProgressReporter;
use crate::remote_init::{init_remote_knots_branch, RemoteInitError};
use crate::replication::{PushSummary, ReplicationService, ReplicationSummary};
use crate::snapshots::{write_snapshots, SnapshotError, SnapshotWriteSummary};
use crate::state_hierarchy::{self, HierarchyKnot, TransitionPlan};
use crate::sync::{SyncError, SyncSummary};
use crate::workflow::{normalize_profile_id, ProfileDefinition, ProfileError, ProfileRegistry};
use crate::workflow_runtime;

const DEFAULT_PROFILE_ID: &str = "autopilot";

pub struct App {
    conn: Connection,
    writer: EventWriter,
    repo_root: PathBuf,
    profile_registry: ProfileRegistry,
    home_override: Option<Option<PathBuf>>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct KnotView {
    pub id: String,
    pub alias: Option<String>,
    pub title: String,
    pub state: String,
    pub updated_at: String,
    pub body: Option<String>,
    pub description: Option<String>,
    pub priority: Option<i64>,
    #[serde(rename = "type")]
    pub knot_type: KnotType,
    pub tags: Vec<String>,
    pub notes: Vec<MetadataEntry>,
    pub handoff_capsules: Vec<MetadataEntry>,
    pub invariants: Vec<Invariant>,
    pub step_history: Vec<StepRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<GateData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease: Option<LeaseData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_id: Option<String>,
    pub profile_id: String,
    pub profile_etag: Option<String>,
    pub deferred_from_state: Option<String>,
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<EdgeView>,
}

#[derive(Debug, Clone, Default)]
pub struct StateActorMetadata {
    pub actor_kind: Option<String>,
    pub agent_name: Option<String>,
    pub agent_model: Option<String>,
    pub agent_version: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct StateCascadeMetadata<'a> {
    root_id: &'a str,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateKnotPatch {
    pub title: Option<String>,
    pub description: Option<String>,
    pub priority: Option<i64>,
    pub status: Option<String>,
    pub knot_type: Option<KnotType>,
    pub add_tags: Vec<String>,
    pub remove_tags: Vec<String>,
    pub add_invariants: Vec<Invariant>,
    pub remove_invariants: Vec<Invariant>,
    pub clear_invariants: bool,
    pub gate_owner_kind: Option<GateOwnerKind>,
    pub gate_failure_modes: Option<BTreeMap<String, Vec<String>>>,
    pub clear_gate_failure_modes: bool,
    pub add_note: Option<MetadataEntryInput>,
    pub add_handoff_capsule: Option<MetadataEntryInput>,
    pub expected_profile_etag: Option<String>,
    pub force: bool,
    pub state_actor: StateActorMetadata,
}

#[derive(Debug, Clone, Default)]
pub struct CreateKnotOptions {
    pub knot_type: KnotType,
    pub gate_data: GateData,
    pub lease_data: LeaseData,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EdgeView {
    pub src: String,
    pub kind: String,
    pub dst: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GateEvaluationResult {
    pub gate: KnotView,
    pub decision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invariant: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reopened: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateDecision {
    Yes,
    No,
}

#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct UserConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_quick_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ColdKnotView {
    pub id: String,
    pub title: String,
    pub state: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PullDriftWarning {
    pub unpushed_event_files: u64,
    pub threshold: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncPolicy {
    Auto,
    Always,
    Never,
}

impl UpdateKnotPatch {
    fn has_changes(&self) -> bool {
        self.title.is_some()
            || self.description.is_some()
            || self.priority.is_some()
            || self.status.is_some()
            || self.knot_type.is_some()
            || !self.add_tags.is_empty()
            || !self.remove_tags.is_empty()
            || !self.add_invariants.is_empty()
            || !self.remove_invariants.is_empty()
            || self.clear_invariants
            || self.gate_owner_kind.is_some()
            || self.gate_failure_modes.is_some()
            || self.clear_gate_failure_modes
            || self.add_note.is_some()
            || self.add_handoff_capsule.is_some()
    }
}

impl App {
    pub fn open(db_path: &str, repo_root: PathBuf) -> Result<Self, AppError> {
        let db = std::path::Path::new(db_path);
        let is_default_path = db
            .components()
            .next()
            .is_some_and(|c| c.as_os_str() == ".knots");
        if is_default_path && !repo_root.join(".knots").exists() {
            return Err(AppError::NotInitialized);
        }
        ensure_parent_dir(db_path)?;
        let conn = db::open_connection(db_path)?;
        let profile_registry = ProfileRegistry::load()?;
        let writer = EventWriter::new(repo_root.clone());
        Ok(Self {
            conn,
            writer,
            repo_root,
            profile_registry,
            home_override: None,
        })
    }

    pub(crate) fn with_home_override(mut self, home: Option<PathBuf>) -> Self {
        self.home_override = Some(home);
        self
    }

    fn repo_lock_path(&self) -> PathBuf {
        self.repo_root
            .join(".knots")
            .join("locks")
            .join("repo.lock")
    }

    fn cache_lock_path(&self) -> PathBuf {
        self.repo_root
            .join(".knots")
            .join("cache")
            .join("cache.lock")
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
        let budget = raw.trim().parse::<u64>().unwrap_or(750);
        Ok(budget)
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
            .map(|profile| profile.id)
            .ok_or_else(|| AppError::InvalidArgument("no profiles are defined".to_string()))
    }

    fn config_path(&self) -> Option<PathBuf> {
        let home = match &self.home_override {
            Some(explicit) => explicit.clone(),
            None => std::env::var_os("HOME").map(PathBuf::from),
        }?;
        Some(home.join(".config").join("knots").join("config.toml"))
    }

    fn read_user_config(&self) -> Result<UserConfig, AppError> {
        let Some(path) = self.config_path() else {
            return Ok(UserConfig::default());
        };
        if !path.exists() {
            return Ok(UserConfig::default());
        }
        let raw = fs::read_to_string(path)?;
        let parsed: UserConfig = toml::from_str(&raw)
            .map_err(|err| AppError::InvalidArgument(format!("invalid profile config: {err}")))?;
        Ok(parsed)
    }

    fn write_user_config(&self, config: &UserConfig) -> Result<(), AppError> {
        let path = self.config_path().ok_or_else(|| {
            AppError::InvalidArgument("unable to resolve $HOME for profile config".to_string())
        })?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let rendered = toml::to_string_pretty(config).map_err(|err| {
            AppError::InvalidArgument(format!("failed to serialize config: {err}"))
        })?;
        fs::write(path, rendered)?;
        Ok(())
    }

    fn resolve_config_profile(&self, raw: &Option<String>) -> Option<String> {
        let raw_id = raw.as_deref()?;
        let profile = self.profile_registry.require(raw_id).ok()?;
        Some(profile.id.clone())
    }

    pub fn default_profile_id(&self) -> Result<String, AppError> {
        let config = self.read_user_config()?;
        if let Some(id) = self.resolve_config_profile(&config.default_profile) {
            return Ok(id);
        }
        self.fallback_profile_id()
    }

    pub fn set_default_profile_id(&self, profile_id: &str) -> Result<String, AppError> {
        let profile = self.profile_registry.require(profile_id)?;
        let mut config = self.read_user_config()?;
        config.default_profile = Some(profile.id.clone());
        self.write_user_config(&config)?;
        Ok(profile.id.clone())
    }

    pub fn default_quick_profile_id(&self) -> Result<String, AppError> {
        let config = self.read_user_config()?;
        if let Some(id) = self.resolve_config_profile(&config.default_quick_profile) {
            return Ok(id);
        }
        // Fallback: first profile with planning_mode == Skipped
        let profiles = self.profile_registry.list();
        for profile in &profiles {
            if profile.planning_mode == crate::workflow::GateMode::Skipped {
                return Ok(profile.id.clone());
            }
        }
        self.fallback_profile_id()
    }

    pub fn set_default_quick_profile_id(&self, profile_id: &str) -> Result<String, AppError> {
        let profile = self.profile_registry.require(profile_id)?;
        let mut config = self.read_user_config()?;
        config.default_quick_profile = Some(profile.id.clone());
        self.write_user_config(&config)?;
        Ok(profile.id.clone())
    }

    fn mark_sync_pending(&self) -> Result<(), AppError> {
        db::set_meta(&self.conn, "sync_pending", "true")?;
        Ok(())
    }

    fn maybe_auto_sync_for_read(&self) -> Result<(), AppError> {
        match self.read_sync_policy()? {
            SyncPolicy::Never => Ok(()),
            SyncPolicy::Always => {
                let _ = self.pull()?;
                Ok(())
            }
            SyncPolicy::Auto => {
                let repo_lock = FileLock::try_acquire(&self.repo_lock_path())?;
                let Some(_repo_guard) = repo_lock else {
                    return self.mark_sync_pending();
                };
                let cache_lock = FileLock::try_acquire(&self.cache_lock_path())?;
                let Some(_cache_guard) = cache_lock else {
                    return self.mark_sync_pending();
                };
                let start = Instant::now();
                if let Err(err) = self.pull_unlocked() {
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
                if start.elapsed().as_millis() > self.read_sync_budget_ms()? as u128 {
                    self.mark_sync_pending()?;
                }
                Ok(())
            }
        }
    }

    fn pull_unlocked(&self) -> Result<SyncSummary, AppError> {
        let service = ReplicationService::new(&self.conn, self.repo_root.clone());
        Ok(service.pull()?)
    }

    fn pull_unlocked_with_progress(
        &self,
        reporter: &mut Option<&mut dyn ProgressReporter>,
    ) -> Result<SyncSummary, AppError> {
        let service = ReplicationService::new(&self.conn, self.repo_root.clone());
        Ok(service.pull_with_progress(reporter)?)
    }

    fn known_knot_ids(&self) -> Result<HashSet<String>, AppError> {
        let mut ids = HashSet::new();
        for record in db::list_knot_hot(&self.conn)? {
            ids.insert(record.id);
        }
        for record in db::list_knot_warm(&self.conn)? {
            ids.insert(record.id);
        }
        for record in db::list_cold_catalog(&self.conn)? {
            ids.insert(record.id);
        }
        Ok(ids)
    }

    fn alias_maps(&self) -> Result<AliasMaps, AppError> {
        let mut ids = self.known_knot_ids()?;
        let parent_edges = db::list_edges_by_kind(&self.conn, "parent_of")?;
        let mut edges = Vec::new();
        for edge in parent_edges {
            ids.insert(edge.src.clone());
            ids.insert(edge.dst.clone());
            edges.push((edge.src, edge.dst));
        }
        Ok(build_alias_maps(ids.into_iter().collect(), &edges))
    }

    fn resolve_knot_token(&self, token: &str) -> Result<String, AppError> {
        if token.trim().is_empty() {
            return Ok(token.to_string());
        }
        let maps = self.alias_maps()?;
        if let Some(id) = maps.alias_to_id.get(token) {
            return Ok(id.clone());
        }

        // Try suffix-based lookup (e.g. "ba0e" → "knots-ba0e")
        let suffix_part = token.split('.').next().unwrap_or(token);
        let mut suffix_matches = maps
            .id_to_alias
            .keys()
            .filter_map(|id| {
                id.rsplit_once('-')
                    .filter(|(_, s)| *s == suffix_part)
                    .map(|_| id.clone())
            })
            .collect::<Vec<_>>();

        // For a plain suffix with no dots, return the match directly
        if !token.contains('.') {
            return match suffix_matches.len() {
                0 => Ok(token.to_string()),
                1 => Ok(suffix_matches.remove(0)),
                _ => {
                    suffix_matches.sort();
                    Err(AppError::InvalidArgument(format!(
                        "ambiguous knot id '{}'; matches: {}",
                        token,
                        suffix_matches.join(", ")
                    )))
                }
            };
        }

        // Partial hierarchical alias (e.g. "ba0e.2" → "knots-ba0e.2")
        let dot_tail = &token[suffix_part.len()..];
        if suffix_matches.is_empty() {
            return Ok(token.to_string());
        }
        let mut resolved: Vec<String> = suffix_matches
            .iter()
            .filter_map(|pfx| {
                let full = format!("{}{}", pfx, dot_tail);
                maps.alias_to_id.get(&full).cloned()
            })
            .collect();
        resolved.sort();
        resolved.dedup();
        match resolved.len() {
            0 => Err(AppError::NotFound(token.to_string())),
            1 => Ok(resolved.remove(0)),
            _ => Err(AppError::InvalidArgument(format!(
                "ambiguous knot alias '{}'; matches: {}",
                token,
                resolved.join(", ")
            ))),
        }
    }

    fn with_alias_maps(knot: KnotView, maps: &AliasMaps) -> KnotView {
        let mut knot = knot;
        let alias = maps.id_to_alias.get(&knot.id).cloned();
        knot.alias = alias.filter(|value| value != &knot.id);
        knot
    }

    fn apply_aliases_to_knots(&self, knots: Vec<KnotView>) -> Result<Vec<KnotView>, AppError> {
        let maps = self.alias_maps()?;
        Ok(knots
            .into_iter()
            .map(|knot| Self::with_alias_maps(knot, &maps))
            .collect())
    }

    fn apply_alias_to_knot(&self, knot: KnotView) -> Result<KnotView, AppError> {
        let maps = self.alias_maps()?;
        Ok(Self::with_alias_maps(knot, &maps))
    }

    fn next_knot_id(&self) -> Result<String, AppError> {
        let existing = self.known_knot_ids()?;
        Ok(generate_knot_id(&self.repo_root, |candidate| {
            existing.contains(candidate)
        }))
    }

    fn resolve_profile_for_record<'a>(
        &'a self,
        record: &KnotCacheRecord,
    ) -> Result<&'a ProfileDefinition, AppError> {
        let profile_id = non_empty(record.profile_id.as_str()).ok_or_else(|| {
            AppError::InvalidArgument(format!("knot '{}' is missing profile_id", record.id))
        })?;
        Ok(self.profile_registry.require(&profile_id)?)
    }

    pub fn create_knot(
        &self,
        title: &str,
        body: Option<&str>,
        initial_state: Option<&str>,
        profile_id: Option<&str>,
    ) -> Result<KnotView, AppError> {
        self.create_knot_with_options(
            title,
            body,
            initial_state,
            profile_id,
            CreateKnotOptions::default(),
        )
    }

    pub fn create_knot_with_options(
        &self,
        title: &str,
        body: Option<&str>,
        initial_state: Option<&str>,
        profile_id: Option<&str>,
        options: CreateKnotOptions,
    ) -> Result<KnotView, AppError> {
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        let default_profile = if profile_id.is_none() {
            Some(self.default_profile_id()?)
        } else {
            None
        };
        let profile = self
            .profile_registry
            .resolve(profile_id.or(default_profile.as_deref()))?;
        let state = if let Some(requested) = non_empty(initial_state.unwrap_or("")) {
            normalize_state_input(&requested)?
        } else {
            workflow_runtime::initial_state(options.knot_type, profile)
        };
        require_state_for_knot_type(options.knot_type, profile, &state)?;
        let knot_id = self.next_knot_id()?;
        let occurred_at = now_utc_rfc3339();
        let terminal = workflow_runtime::is_terminal_state(
            &self.profile_registry,
            profile.id.as_str(),
            options.knot_type,
            &state,
        )?;

        let full_event = FullEvent::with_identity(
            new_event_id(),
            occurred_at.clone(),
            knot_id.clone(),
            FullEventKind::KnotCreated.as_str(),
            json!({
                "title": title,
                "state": state.as_str(),
                "profile_id": profile.id.as_str(),
                "body": body,
                "type": options.knot_type.as_str(),
                "gate": &options.gate_data,
            }),
        );

        let index_event_id = new_event_id();
        let idx_event = IndexEvent::with_identity(
            index_event_id.clone(),
            occurred_at.clone(),
            IndexEventKind::KnotHead.as_str(),
            build_knot_head_data(KnotHeadData {
                knot_id: &knot_id,
                title,
                state: state.as_str(),
                profile_id: profile.id.as_str(),
                updated_at: &occurred_at,
                terminal,
                deferred_from_state: None,
                invariants: &[],
                knot_type: options.knot_type,
                gate_data: &options.gate_data,
            }),
        );

        self.writer.write(&EventRecord::full(full_event))?;
        if options.knot_type == KnotType::Lease {
            let lease_event = FullEvent::new(
                knot_id.clone(),
                FullEventKind::KnotLeaseDataSet,
                json!({"lease_data": &options.lease_data}),
            );
            self.writer.write(&EventRecord::full(lease_event))?;
        }
        self.writer.write(&EventRecord::index(idx_event))?;

        db::upsert_knot_hot(
            &self.conn,
            &UpsertKnotHot {
                id: &knot_id,
                title,
                state: state.as_str(),
                updated_at: &occurred_at,
                body,
                description: body,
                priority: None,
                knot_type: Some(options.knot_type.as_str()),
                tags: &[],
                notes: &[],
                handoff_capsules: &[],
                invariants: &[],
                step_history: &[],
                gate_data: &options.gate_data,
                lease_data: &options.lease_data,
                lease_id: None,
                profile_id: profile.id.as_str(),
                profile_etag: Some(&index_event_id),
                deferred_from_state: None,
                created_at: Some(&occurred_at),
            },
        )?;
        let record = db::get_knot_hot(&self.conn, &knot_id)?
            .ok_or_else(|| AppError::NotFound(knot_id.clone()))?;
        self.apply_alias_to_knot(KnotView::from(record))
    }

    pub fn set_profile(
        &self,
        id: &str,
        profile_id: &str,
        state: &str,
        expected_profile_etag: Option<&str>,
    ) -> Result<KnotView, AppError> {
        let id = self.resolve_knot_token(id)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        let current =
            db::get_knot_hot(&self.conn, &id)?.ok_or_else(|| AppError::NotFound(id.clone()))?;
        ensure_profile_etag(&current, expected_profile_etag)?;

        let profile = self.profile_registry.require(profile_id)?;
        let next_state = normalize_state_input(state)?;
        require_state_for_knot_type(
            parse_knot_type(current.knot_type.as_deref()),
            profile,
            &next_state,
        )?;

        let current_profile_id = canonical_profile_id(&current.profile_id);
        if current_profile_id == profile.id && current.state == next_state {
            return self.apply_alias_to_knot(KnotView::from(current));
        }

        let deferred_from_state = if next_state == "deferred" && current.state != "deferred" {
            Some(current.state.clone())
        } else if current.state == "deferred" && next_state != "deferred" {
            None
        } else {
            current.deferred_from_state.clone()
        };

        let occurred_at = now_utc_rfc3339();
        let mut full_event = FullEvent::with_identity(
            new_event_id(),
            occurred_at.clone(),
            id.clone(),
            FullEventKind::KnotProfileSet.as_str(),
            json!({
                "from_profile_id": current_profile_id,
                "to_profile_id": profile.id,
                "from_state": current.state,
                "to_state": next_state,
                "deferred_from_state": deferred_from_state,
            }),
        );
        if let Some(expected) = expected_profile_etag {
            full_event = full_event.with_precondition(expected);
        }

        let index_event_id = new_event_id();
        let knot_type = parse_knot_type(current.knot_type.as_deref());
        let terminal = workflow_runtime::is_terminal_state(
            &self.profile_registry,
            profile.id.as_str(),
            knot_type,
            &next_state,
        )?;
        let mut idx_event = IndexEvent::with_identity(
            index_event_id.clone(),
            occurred_at.clone(),
            IndexEventKind::KnotHead.as_str(),
            build_knot_head_data(KnotHeadData {
                knot_id: &id,
                title: &current.title,
                state: &next_state,
                profile_id: profile.id.as_str(),
                updated_at: &occurred_at,
                terminal,
                deferred_from_state: deferred_from_state.as_deref(),
                invariants: &current.invariants,
                knot_type,
                gate_data: &current.gate_data,
            }),
        );
        if let Some(expected) = expected_profile_etag {
            idx_event = idx_event.with_precondition(expected);
        }

        self.writer.write(&EventRecord::full(full_event))?;
        self.writer.write(&EventRecord::index(idx_event))?;

        db::upsert_knot_hot(
            &self.conn,
            &UpsertKnotHot {
                id: &id,
                title: &current.title,
                state: &next_state,
                updated_at: &occurred_at,
                body: current.body.as_deref(),
                description: current.description.as_deref(),
                priority: current.priority,
                knot_type: current.knot_type.as_deref(),
                tags: &current.tags,
                notes: &current.notes,
                handoff_capsules: &current.handoff_capsules,
                invariants: &current.invariants,
                step_history: &current.step_history,
                gate_data: &current.gate_data,
                lease_data: &current.lease_data,
                lease_id: current.lease_id.as_deref(),
                profile_id: &profile.id,
                profile_etag: Some(&index_event_id),
                deferred_from_state: deferred_from_state.as_deref(),
                created_at: current.created_at.as_deref(),
            },
        )?;
        let updated =
            db::get_knot_hot(&self.conn, &id)?.ok_or_else(|| AppError::NotFound(id.clone()))?;
        self.apply_alias_to_knot(KnotView::from(updated))
    }

    #[allow(dead_code)]
    pub fn set_state(
        &self,
        id: &str,
        next_state: &str,
        force: bool,
        expected_profile_etag: Option<&str>,
    ) -> Result<KnotView, AppError> {
        self.set_state_with_actor(
            id,
            next_state,
            force,
            expected_profile_etag,
            StateActorMetadata::default(),
        )
    }

    pub(crate) fn reconcile_terminal_parent_state(
        &self,
        id: &str,
        next_state: &str,
    ) -> Result<KnotView, AppError> {
        let id = self.resolve_knot_token(id)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        let current =
            db::get_knot_hot(&self.conn, &id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
        let next_state = normalize_state_input(next_state)?;
        let updated = self.reconcile_terminal_parent_state_locked(&current, &next_state)?;
        if self.transitioned_to_terminal_resolution_state(&current, &updated)? {
            self.auto_resolve_terminal_parents_locked([updated.id.as_str()])?;
        }
        self.apply_alias_to_knot(KnotView::from(updated))
    }

    pub fn set_state_with_actor(
        &self,
        id: &str,
        next_state: &str,
        force: bool,
        expected_profile_etag: Option<&str>,
        state_actor: StateActorMetadata,
    ) -> Result<KnotView, AppError> {
        self.set_state_with_actor_and_options(
            id,
            next_state,
            force,
            expected_profile_etag,
            state_actor,
            false,
        )
    }

    pub(crate) fn set_state_with_actor_and_options(
        &self,
        id: &str,
        next_state: &str,
        force: bool,
        expected_profile_etag: Option<&str>,
        state_actor: StateActorMetadata,
        approve_terminal_cascade: bool,
    ) -> Result<KnotView, AppError> {
        let id = self.resolve_knot_token(id)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        let current =
            db::get_knot_hot(&self.conn, &id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
        ensure_profile_etag(&current, expected_profile_etag)?;
        let next = normalize_state_input(next_state)?;
        let updated = self.apply_state_transition_locked(
            &current,
            &next,
            force,
            expected_profile_etag,
            &state_actor,
            approve_terminal_cascade,
        )?;
        if self.transitioned_to_terminal_resolution_state(&current, &updated)? {
            self.auto_resolve_terminal_parents_locked([updated.id.as_str()])?;
        }
        self.apply_alias_to_knot(KnotView::from(updated))
    }

    pub fn update_knot(&self, id: &str, patch: UpdateKnotPatch) -> Result<KnotView, AppError> {
        self.update_knot_with_options(id, patch, false)
    }

    pub(crate) fn update_knot_with_options(
        &self,
        id: &str,
        patch: UpdateKnotPatch,
        approve_terminal_cascade: bool,
    ) -> Result<KnotView, AppError> {
        let id = self.resolve_knot_token(id)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        if !patch.has_changes() {
            return Err(AppError::InvalidArgument(
                "update requires at least one field change".to_string(),
            ));
        }

        let mut current =
            db::get_knot_hot(&self.conn, &id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
        ensure_profile_etag(&current, patch.expected_profile_etag.as_deref())?;
        let mut current_precondition = patch.expected_profile_etag.clone();
        let mut title = current.title.clone();
        let mut state = current.state.clone();
        let mut description = current.description.clone();
        let mut body = current.body.clone();
        let mut priority = current.priority;
        let mut knot_type = parse_knot_type(current.knot_type.as_deref());
        let profile = self.resolve_profile_for_record(&current)?;
        let profile_id = profile.id.clone();
        let mut deferred_from_state = current.deferred_from_state.clone();
        let mut tags = current.tags.clone();
        let mut notes = current.notes.clone();
        let mut handoff_capsules = current.handoff_capsules.clone();
        let mut invariants = current.invariants.clone();
        let mut gate_data = current.gate_data.clone();
        let occurred_at = now_utc_rfc3339();
        let mut full_events = Vec::new();

        if let Some(next_state_raw) = patch.status.as_deref() {
            let next_state = normalize_state_input(next_state_raw)?;
            let next_is_terminal = workflow_runtime::is_terminal_state(
                &self.profile_registry,
                &profile_id,
                knot_type,
                &next_state,
            )?;
            if state == "deferred" && next_state != "deferred" && !patch.force && !next_is_terminal
            {
                let expected = deferred_from_state.as_deref().ok_or_else(|| {
                    AppError::InvalidArgument(
                        "deferred knot is missing deferred_from_state provenance".to_string(),
                    )
                })?;
                if expected != next_state {
                    return Err(AppError::InvalidArgument(format!(
                        "deferred knots may only resume to '{}'",
                        expected
                    )));
                }
            } else {
                workflow_runtime::validate_transition(
                    &self.profile_registry,
                    &profile_id,
                    knot_type,
                    &state,
                    &next_state,
                    patch.force,
                )?;
            }
            match state_hierarchy::plan_state_transition(
                &self.conn,
                &current,
                &next_state,
                next_is_terminal,
                approve_terminal_cascade,
            )? {
                TransitionPlan::Allowed if state != next_state => {
                    if next_state == "deferred" && state != "deferred" {
                        deferred_from_state = Some(state.clone());
                    } else if state == "deferred" && next_state != "deferred" {
                        deferred_from_state = None;
                    }
                    state = next_state;
                    let state_event_data = build_state_event_data(
                        &current.state,
                        &state,
                        &profile_id,
                        patch.force,
                        deferred_from_state.as_deref(),
                        &patch.state_actor,
                        None,
                    )?;
                    full_events.push(FullEvent::with_identity(
                        new_event_id(),
                        occurred_at.clone(),
                        id.to_string(),
                        FullEventKind::KnotStateSet.as_str(),
                        state_event_data,
                    ));
                }
                TransitionPlan::Allowed => {}
                TransitionPlan::CascadeTerminal { descendants } => {
                    current = self.cascade_terminal_state_locked(
                        &current,
                        &next_state,
                        patch.expected_profile_etag.as_deref(),
                        &patch.state_actor,
                        &descendants,
                        patch.force,
                    )?;
                    current_precondition = current.profile_etag.clone();
                    title = current.title.clone();
                    state = current.state.clone();
                    description = current.description.clone();
                    body = current.body.clone();
                    priority = current.priority;
                    knot_type = parse_knot_type(current.knot_type.as_deref());
                    deferred_from_state = current.deferred_from_state.clone();
                    tags = current.tags.clone();
                    notes = current.notes.clone();
                    handoff_capsules = current.handoff_capsules.clone();
                    invariants = current.invariants.clone();
                    gate_data = current.gate_data.clone();
                }
            }
        }

        if let Some(next_title_raw) = patch.title.as_deref() {
            let next_title = next_title_raw.trim();
            if next_title.is_empty() {
                return Err(AppError::InvalidArgument(
                    "title cannot be empty".to_string(),
                ));
            }
            if next_title != title {
                full_events.push(FullEvent::with_identity(
                    new_event_id(),
                    occurred_at.clone(),
                    id.to_string(),
                    FullEventKind::KnotTitleSet.as_str(),
                    json!({
                        "from": &title,
                        "to": next_title,
                    }),
                ));
                title = next_title.to_string();
            }
        }

        if let Some(next_description_raw) = patch.description.as_deref() {
            let next_description = non_empty(next_description_raw);
            if next_description != description {
                full_events.push(FullEvent::with_identity(
                    new_event_id(),
                    occurred_at.clone(),
                    id.to_string(),
                    FullEventKind::KnotDescriptionSet.as_str(),
                    json!({
                        "description": next_description,
                    }),
                ));
                description = next_description;
                body = description.clone();
            }
        }

        if let Some(next_priority) = patch.priority {
            if !(0..=4).contains(&next_priority) {
                return Err(AppError::InvalidArgument(
                    "priority must be between 0 and 4".to_string(),
                ));
            }
            if priority != Some(next_priority) {
                full_events.push(FullEvent::with_identity(
                    new_event_id(),
                    occurred_at.clone(),
                    id.to_string(),
                    FullEventKind::KnotPrioritySet.as_str(),
                    json!({
                        "priority": next_priority,
                    }),
                ));
                priority = Some(next_priority);
            }
        }

        if let Some(next_type) = patch.knot_type {
            if next_type != knot_type {
                full_events.push(FullEvent::with_identity(
                    new_event_id(),
                    occurred_at.clone(),
                    id.to_string(),
                    FullEventKind::KnotTypeSet.as_str(),
                    json!({
                        "type": next_type.as_str(),
                    }),
                ));
                knot_type = next_type;
            }
        }

        if let Some(owner_kind) = patch.gate_owner_kind {
            gate_data.owner_kind = owner_kind;
        }
        if patch.clear_gate_failure_modes {
            gate_data.failure_modes.clear();
        }
        if let Some(failure_modes) = patch.gate_failure_modes.clone() {
            gate_data.failure_modes = failure_modes;
        }
        if patch.gate_owner_kind.is_some()
            || patch.clear_gate_failure_modes
            || patch.gate_failure_modes.is_some()
        {
            require_gate_metadata_scope(knot_type)?;
            full_events.push(FullEvent::with_identity(
                new_event_id(),
                occurred_at.clone(),
                id.to_string(),
                FullEventKind::KnotGateDataSet.as_str(),
                json!({ "gate": &gate_data }),
            ));
        }

        for tag in &patch.add_tags {
            let normalized = normalize_tag(tag);
            if normalized.is_empty() {
                continue;
            }
            if !tags.iter().any(|existing| existing == &normalized) {
                tags.push(normalized.clone());
                full_events.push(FullEvent::with_identity(
                    new_event_id(),
                    occurred_at.clone(),
                    id.to_string(),
                    FullEventKind::KnotTagAdd.as_str(),
                    json!({ "tag": normalized }),
                ));
            }
        }

        for tag in &patch.remove_tags {
            let normalized = normalize_tag(tag);
            if normalized.is_empty() {
                continue;
            }
            if tags.iter().any(|existing| existing == &normalized) {
                tags.retain(|existing| existing != &normalized);
                full_events.push(FullEvent::with_identity(
                    new_event_id(),
                    occurred_at.clone(),
                    id.to_string(),
                    FullEventKind::KnotTagRemove.as_str(),
                    json!({ "tag": normalized }),
                ));
            }
        }

        if patch.clear_invariants {
            invariants.clear();
        }
        for invariant in &patch.add_invariants {
            if !invariants.iter().any(|existing| existing == invariant) {
                invariants.push(invariant.clone());
            }
        }
        for invariant in &patch.remove_invariants {
            invariants.retain(|existing| existing != invariant);
        }
        if invariants != current.invariants {
            full_events.push(FullEvent::with_identity(
                new_event_id(),
                occurred_at.clone(),
                id.to_string(),
                FullEventKind::KnotInvariantsSet.as_str(),
                json!({
                    "invariants": &invariants,
                }),
            ));
        }

        if let Some(input) = patch.add_note {
            let entry = metadata_entry_from_input(input, &occurred_at)?;
            if !notes
                .iter()
                .any(|existing| existing.entry_id == entry.entry_id)
            {
                notes.push(entry.clone());
                full_events.push(FullEvent::with_identity(
                    new_event_id(),
                    occurred_at.clone(),
                    id.to_string(),
                    FullEventKind::KnotNoteAdded.as_str(),
                    json!({
                        "entry_id": entry.entry_id,
                        "content": entry.content,
                        "username": entry.username,
                        "datetime": entry.datetime,
                        "agentname": entry.agentname,
                        "model": entry.model,
                        "version": entry.version,
                    }),
                ));
            }
        }

        if let Some(input) = patch.add_handoff_capsule {
            let entry = metadata_entry_from_input(input, &occurred_at)?;
            if !handoff_capsules
                .iter()
                .any(|existing| existing.entry_id == entry.entry_id)
            {
                handoff_capsules.push(entry.clone());
                full_events.push(FullEvent::with_identity(
                    new_event_id(),
                    occurred_at.clone(),
                    id.to_string(),
                    FullEventKind::KnotHandoffCapsuleAdded.as_str(),
                    json!({
                        "entry_id": entry.entry_id,
                        "content": entry.content,
                        "username": entry.username,
                        "datetime": entry.datetime,
                        "agentname": entry.agentname,
                        "model": entry.model,
                        "version": entry.version,
                    }),
                ));
            }
        }

        if full_events.is_empty() {
            return self.apply_alias_to_knot(KnotView::from(current));
        }

        for mut event in full_events {
            if let Some(expected) = current_precondition.as_deref() {
                event = event.with_precondition(expected);
            }
            self.writer.write(&EventRecord::full(event))?;
        }

        let terminal = workflow_runtime::is_terminal_state(
            &self.profile_registry,
            &profile_id,
            knot_type,
            &state,
        )?;
        let index_event_id = new_event_id();
        let mut idx_event = IndexEvent::with_identity(
            index_event_id.clone(),
            occurred_at.clone(),
            IndexEventKind::KnotHead.as_str(),
            build_knot_head_data(KnotHeadData {
                knot_id: &id,
                title: &title,
                state: &state,
                profile_id: &profile_id,
                updated_at: &occurred_at,
                terminal,
                deferred_from_state: deferred_from_state.as_deref(),
                invariants: &invariants,
                knot_type,
                gate_data: &gate_data,
            }),
        );
        if let Some(expected) = current_precondition.as_deref() {
            idx_event = idx_event.with_precondition(expected);
        }
        self.writer.write(&EventRecord::index(idx_event))?;

        db::upsert_knot_hot(
            &self.conn,
            &UpsertKnotHot {
                id: &id,
                title: &title,
                state: &state,
                updated_at: &occurred_at,
                body: body.as_deref(),
                description: description.as_deref(),
                priority,
                knot_type: Some(knot_type.as_str()),
                tags: &tags,
                notes: &notes,
                handoff_capsules: &handoff_capsules,
                invariants: &invariants,
                step_history: &apply_step_transition(
                    &current.step_history,
                    &current.state,
                    &state,
                    &occurred_at,
                    &patch.state_actor,
                ),
                gate_data: &gate_data,
                lease_data: &current.lease_data,
                lease_id: current.lease_id.as_deref(),
                profile_id: &profile_id,
                profile_etag: Some(&index_event_id),
                deferred_from_state: deferred_from_state.as_deref(),
                created_at: current.created_at.as_deref(),
            },
        )?;
        let updated =
            db::get_knot_hot(&self.conn, &id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
        if self.transitioned_to_terminal_resolution_state(&current, &updated)? {
            self.auto_resolve_terminal_parents_locked([updated.id.as_str()])?;
        }
        self.apply_alias_to_knot(KnotView::from(updated))
    }

    fn apply_state_transition_locked(
        &self,
        current: &KnotCacheRecord,
        next_state: &str,
        force: bool,
        expected_profile_etag: Option<&str>,
        state_actor: &StateActorMetadata,
        approve_terminal_cascade: bool,
    ) -> Result<KnotCacheRecord, AppError> {
        let profile = self.resolve_profile_for_record(current)?;
        let knot_type = parse_knot_type(current.knot_type.as_deref());
        let next_is_terminal = workflow_runtime::is_terminal_state(
            &self.profile_registry,
            &profile.id,
            knot_type,
            next_state,
        )?;
        if current.state == "deferred" && next_state != "deferred" && !force && !next_is_terminal {
            let expected = current.deferred_from_state.as_deref().ok_or_else(|| {
                AppError::InvalidArgument(
                    "deferred knot is missing deferred_from_state provenance".to_string(),
                )
            })?;
            if expected != next_state {
                return Err(AppError::InvalidArgument(format!(
                    "deferred knots may only resume to '{}'",
                    expected
                )));
            }
        } else {
            workflow_runtime::validate_transition(
                &self.profile_registry,
                &profile.id,
                knot_type,
                &current.state,
                next_state,
                force,
            )?;
        }

        match state_hierarchy::plan_state_transition(
            &self.conn,
            current,
            next_state,
            next_is_terminal,
            approve_terminal_cascade,
        )? {
            TransitionPlan::Allowed => self.write_state_change_locked(
                current,
                next_state,
                force,
                expected_profile_etag,
                state_actor,
                None,
            ),
            TransitionPlan::CascadeTerminal { descendants } => self.cascade_terminal_state_locked(
                current,
                next_state,
                expected_profile_etag,
                state_actor,
                &descendants,
                force,
            ),
        }
    }

    fn cascade_terminal_state_locked(
        &self,
        current: &KnotCacheRecord,
        next_state: &str,
        expected_profile_etag: Option<&str>,
        state_actor: &StateActorMetadata,
        descendants: &[HierarchyKnot],
        force_root: bool,
    ) -> Result<KnotCacheRecord, AppError> {
        let cascade = StateCascadeMetadata {
            root_id: &current.id,
        };
        let mut changed_terminal_ids = Vec::new();
        for descendant in descendants {
            let Some(descendant_record) = db::get_knot_hot(&self.conn, &descendant.id)? else {
                continue;
            };
            if state_hierarchy::is_terminal_state(&descendant_record.state)? {
                continue;
            }
            self.write_state_change_locked(
                &descendant_record,
                next_state,
                false,
                None,
                state_actor,
                Some(cascade),
            )?;
            changed_terminal_ids.push(descendant_record.id);
        }

        let updated = self.write_state_change_locked(
            current,
            next_state,
            force_root,
            expected_profile_etag,
            state_actor,
            Some(cascade),
        )?;
        changed_terminal_ids.push(updated.id.clone());
        self.auto_resolve_terminal_parents_locked(changed_terminal_ids.iter().map(String::as_str))?;
        Ok(updated)
    }

    fn write_state_change_locked(
        &self,
        current: &KnotCacheRecord,
        next_state: &str,
        force: bool,
        expected_profile_etag: Option<&str>,
        state_actor: &StateActorMetadata,
        cascade: Option<StateCascadeMetadata<'_>>,
    ) -> Result<KnotCacheRecord, AppError> {
        let profile = self.resolve_profile_for_record(current)?;
        let profile_id = profile.id.clone();
        let knot_type = parse_knot_type(current.knot_type.as_deref());
        let deferred_from_state = next_deferred_from_state(current, next_state);
        let occurred_at = now_utc_rfc3339();
        let state_event_data = build_state_event_data(
            &current.state,
            next_state,
            &profile_id,
            force,
            deferred_from_state.as_deref(),
            state_actor,
            cascade,
        )?;
        let mut full_event = FullEvent::with_identity(
            new_event_id(),
            occurred_at.clone(),
            current.id.clone(),
            FullEventKind::KnotStateSet.as_str(),
            state_event_data,
        );
        if let Some(expected) = expected_profile_etag {
            full_event = full_event.with_precondition(expected);
        }

        let index_event_id = new_event_id();
        let terminal = workflow_runtime::is_terminal_state(
            &self.profile_registry,
            &profile_id,
            knot_type,
            next_state,
        )?;
        let mut idx_event = IndexEvent::with_identity(
            index_event_id.clone(),
            occurred_at.clone(),
            IndexEventKind::KnotHead.as_str(),
            build_knot_head_data(KnotHeadData {
                knot_id: &current.id,
                title: &current.title,
                state: next_state,
                profile_id: &profile_id,
                updated_at: &occurred_at,
                terminal,
                deferred_from_state: deferred_from_state.as_deref(),
                invariants: &current.invariants,
                knot_type,
                gate_data: &current.gate_data,
            }),
        );
        if let Some(expected) = expected_profile_etag {
            idx_event = idx_event.with_precondition(expected);
        }

        self.writer.write(&EventRecord::full(full_event))?;
        self.writer.write(&EventRecord::index(idx_event))?;

        let updated_step_history = apply_step_transition(
            &current.step_history,
            &current.state,
            next_state,
            &occurred_at,
            state_actor,
        );

        db::upsert_knot_hot(
            &self.conn,
            &UpsertKnotHot {
                id: &current.id,
                title: &current.title,
                state: next_state,
                updated_at: &occurred_at,
                body: current.body.as_deref(),
                description: current.description.as_deref(),
                priority: current.priority,
                knot_type: current.knot_type.as_deref(),
                tags: &current.tags,
                notes: &current.notes,
                handoff_capsules: &current.handoff_capsules,
                invariants: &current.invariants,
                step_history: &updated_step_history,
                gate_data: &current.gate_data,
                lease_data: &current.lease_data,
                lease_id: current.lease_id.as_deref(),
                profile_id: &profile_id,
                profile_etag: Some(&index_event_id),
                deferred_from_state: deferred_from_state.as_deref(),
                created_at: current.created_at.as_deref(),
            },
        )?;

        db::get_knot_hot(&self.conn, &current.id)?
            .ok_or_else(|| AppError::NotFound(current.id.clone()))
    }

    fn reconcile_terminal_parent_state_locked(
        &self,
        current: &KnotCacheRecord,
        next_state: &str,
    ) -> Result<KnotCacheRecord, AppError> {
        self.write_state_change_locked(
            current,
            next_state,
            true,
            None,
            &StateActorMetadata::default(),
            None,
        )
    }

    fn auto_resolve_terminal_parents_locked<'a>(
        &self,
        knot_ids: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), AppError> {
        let mut pending = knot_ids
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<String>>();
        let mut seen = HashSet::new();

        while let Some(knot_id) = pending.pop() {
            let resolutions =
                state_hierarchy::find_ancestor_terminal_resolutions(&self.conn, &knot_id)?;
            for resolution in resolutions {
                if !seen.insert(resolution.parent.id.clone()) {
                    continue;
                }
                let Some(parent) = db::get_knot_hot(&self.conn, &resolution.parent.id)? else {
                    continue;
                };
                if !state_hierarchy::is_terminal_resolution_state(&parent.state)? {
                    self.reconcile_terminal_parent_state_locked(&parent, &resolution.target_state)?;
                    pending.push(parent.id);
                }
            }
        }

        Ok(())
    }

    fn transitioned_to_terminal_resolution_state(
        &self,
        current: &KnotCacheRecord,
        updated: &KnotCacheRecord,
    ) -> Result<bool, AppError> {
        Ok(
            !state_hierarchy::is_terminal_resolution_state(&current.state)?
                && state_hierarchy::is_terminal_resolution_state(&updated.state)?,
        )
    }

    fn append_gate_failure_metadata_locked(
        &self,
        current: &KnotCacheRecord,
        gate_id: &str,
        invariant: &str,
        state_actor: &StateActorMetadata,
    ) -> Result<KnotCacheRecord, AppError> {
        let occurred_at = now_utc_rfc3339();
        let message = format!(
            "Gate {} failed invariant '{}' and reopened this knot for planning.",
            gate_id, invariant
        );
        let note = metadata_entry_from_input(
            MetadataEntryInput {
                content: message.clone(),
                agentname: state_actor.agent_name.clone(),
                model: state_actor.agent_model.clone(),
                version: state_actor.agent_version.clone(),
                ..Default::default()
            },
            &occurred_at,
        )?;
        let handoff = metadata_entry_from_input(
            MetadataEntryInput {
                content: message,
                agentname: state_actor.agent_name.clone(),
                model: state_actor.agent_model.clone(),
                version: state_actor.agent_version.clone(),
                ..Default::default()
            },
            &occurred_at,
        )?;
        let mut note_event = FullEvent::with_identity(
            new_event_id(),
            occurred_at.clone(),
            current.id.clone(),
            FullEventKind::KnotNoteAdded.as_str(),
            json!({
                "entry_id": note.entry_id,
                "content": note.content,
                "username": note.username,
                "datetime": note.datetime,
                "agentname": note.agentname,
                "model": note.model,
                "version": note.version,
            }),
        );
        let mut handoff_event = FullEvent::with_identity(
            new_event_id(),
            occurred_at.clone(),
            current.id.clone(),
            FullEventKind::KnotHandoffCapsuleAdded.as_str(),
            json!({
                "entry_id": handoff.entry_id,
                "content": handoff.content,
                "username": handoff.username,
                "datetime": handoff.datetime,
                "agentname": handoff.agentname,
                "model": handoff.model,
                "version": handoff.version,
            }),
        );
        if let Some(expected) = current.profile_etag.as_deref() {
            note_event = note_event.with_precondition(expected);
            handoff_event = handoff_event.with_precondition(expected);
        }
        self.writer.write(&EventRecord::full(note_event))?;
        self.writer.write(&EventRecord::full(handoff_event))?;

        let mut notes = current.notes.clone();
        notes.push(note);
        let mut handoff_capsules = current.handoff_capsules.clone();
        handoff_capsules.push(handoff);
        let profile = self.resolve_profile_for_record(current)?;
        let knot_type = parse_knot_type(current.knot_type.as_deref());
        let terminal = workflow_runtime::is_terminal_state(
            &self.profile_registry,
            profile.id.as_str(),
            knot_type,
            &current.state,
        )?;
        let index_event_id = new_event_id();
        let mut idx_event = IndexEvent::with_identity(
            index_event_id.clone(),
            occurred_at.clone(),
            IndexEventKind::KnotHead.as_str(),
            build_knot_head_data(KnotHeadData {
                knot_id: &current.id,
                title: &current.title,
                state: &current.state,
                profile_id: profile.id.as_str(),
                updated_at: &occurred_at,
                terminal,
                deferred_from_state: current.deferred_from_state.as_deref(),
                invariants: &current.invariants,
                knot_type,
                gate_data: &current.gate_data,
            }),
        );
        if let Some(expected) = current.profile_etag.as_deref() {
            idx_event = idx_event.with_precondition(expected);
        }
        self.writer.write(&EventRecord::index(idx_event))?;
        db::upsert_knot_hot(
            &self.conn,
            &UpsertKnotHot {
                id: &current.id,
                title: &current.title,
                state: &current.state,
                updated_at: &occurred_at,
                body: current.body.as_deref(),
                description: current.description.as_deref(),
                priority: current.priority,
                knot_type: current.knot_type.as_deref(),
                tags: &current.tags,
                notes: &notes,
                handoff_capsules: &handoff_capsules,
                invariants: &current.invariants,
                step_history: &current.step_history,
                gate_data: &current.gate_data,
                lease_data: &current.lease_data,
                lease_id: current.lease_id.as_deref(),
                profile_id: profile.id.as_str(),
                profile_etag: Some(&index_event_id),
                deferred_from_state: current.deferred_from_state.as_deref(),
                created_at: current.created_at.as_deref(),
            },
        )?;
        db::get_knot_hot(&self.conn, &current.id)?
            .ok_or_else(|| AppError::NotFound(current.id.clone()))
    }

    pub fn set_lease_id(&self, knot_id: &str, lease_id: Option<&str>) -> Result<(), AppError> {
        let id = self.resolve_knot_token(knot_id)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        let record =
            db::get_knot_hot(&self.conn, &id)?.ok_or_else(|| AppError::NotFound(id.clone()))?;
        let event = FullEvent::new(
            id.clone(),
            FullEventKind::KnotLeaseIdSet,
            json!({ "lease_id": lease_id }),
        );
        self.writer.write(&EventRecord::full(event))?;
        db::upsert_knot_hot(
            &self.conn,
            &UpsertKnotHot {
                id: &record.id,
                title: &record.title,
                state: &record.state,
                updated_at: &record.updated_at,
                body: record.body.as_deref(),
                description: record.description.as_deref(),
                priority: record.priority,
                knot_type: record.knot_type.as_deref(),
                tags: &record.tags,
                notes: &record.notes,
                handoff_capsules: &record.handoff_capsules,
                invariants: &record.invariants,
                step_history: &record.step_history,
                gate_data: &record.gate_data,
                lease_data: &record.lease_data,
                lease_id,
                profile_id: &record.profile_id,
                profile_etag: record.profile_etag.as_deref(),
                deferred_from_state: record.deferred_from_state.as_deref(),
                created_at: record.created_at.as_deref(),
            },
        )?;
        Ok(())
    }

    pub fn list_knots(&self) -> Result<Vec<KnotView>, AppError> {
        self.maybe_auto_sync_for_read()?;
        let knots = db::list_knot_hot(&self.conn)?
            .into_iter()
            .map(KnotView::from)
            .collect();
        self.apply_aliases_to_knots(knots)
    }

    pub fn show_knot(&self, id: &str) -> Result<Option<KnotView>, AppError> {
        let id = self.resolve_knot_token(id)?;
        self.maybe_auto_sync_for_read()?;
        if let Some(knot) = db::get_knot_hot(&self.conn, &id)? {
            let mut view = self.apply_alias_to_knot(KnotView::from(knot))?;
            let edges = db::list_edges(&self.conn, &id, db::EdgeDirection::Both)?;
            view.edges = edges.into_iter().map(EdgeView::from).collect();
            return Ok(Some(view));
        }
        self.rehydrate(&id)
    }

    pub fn step_annotate(&self, id: &str, actor: &StepActorInfo) -> Result<KnotView, AppError> {
        let id = self.resolve_knot_token(id)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        let current =
            db::get_knot_hot(&self.conn, &id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
        if !current.step_history.iter().any(|r| r.is_active()) {
            return Err(AppError::InvalidArgument(
                "no active step to annotate".to_string(),
            ));
        }
        let occurred_at = now_utc_rfc3339();
        let updated_history = annotate_step_history(&current.step_history, actor, &occurred_at);
        let profile = self.resolve_profile_for_record(&current)?;
        let knot_type = parse_knot_type(current.knot_type.as_deref());
        let terminal = workflow_runtime::is_terminal_state(
            &self.profile_registry,
            profile.id.as_str(),
            knot_type,
            &current.state,
        )?;
        let index_event_id = new_event_id();
        let idx_event = IndexEvent::with_identity(
            index_event_id.clone(),
            occurred_at.clone(),
            IndexEventKind::KnotHead.as_str(),
            build_knot_head_data(KnotHeadData {
                knot_id: &id,
                title: &current.title,
                state: &current.state,
                profile_id: profile.id.as_str(),
                updated_at: &occurred_at,
                terminal,
                deferred_from_state: current.deferred_from_state.as_deref(),
                invariants: &current.invariants,
                knot_type,
                gate_data: &current.gate_data,
            }),
        );
        self.writer.write(&EventRecord::index(idx_event))?;
        db::upsert_knot_hot(
            &self.conn,
            &UpsertKnotHot {
                id: &id,
                title: &current.title,
                state: &current.state,
                updated_at: &occurred_at,
                body: current.body.as_deref(),
                description: current.description.as_deref(),
                priority: current.priority,
                knot_type: current.knot_type.as_deref(),
                tags: &current.tags,
                notes: &current.notes,
                handoff_capsules: &current.handoff_capsules,
                invariants: &current.invariants,
                step_history: &updated_history,
                gate_data: &current.gate_data,
                lease_data: &current.lease_data,
                lease_id: current.lease_id.as_deref(),
                profile_id: &profile.id,
                profile_etag: Some(&index_event_id),
                deferred_from_state: current.deferred_from_state.as_deref(),
                created_at: current.created_at.as_deref(),
            },
        )?;
        let updated =
            db::get_knot_hot(&self.conn, &id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
        self.apply_alias_to_knot(KnotView::from(updated))
    }

    pub fn evaluate_gate(
        &self,
        id: &str,
        decision: GateDecision,
        invariant: Option<&str>,
        state_actor: StateActorMetadata,
    ) -> Result<GateEvaluationResult, AppError> {
        let id = self.resolve_knot_token(id)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        let current =
            db::get_knot_hot(&self.conn, &id)?.ok_or_else(|| AppError::NotFound(id.clone()))?;
        if parse_knot_type(current.knot_type.as_deref()) != KnotType::Gate {
            return Err(AppError::InvalidArgument(format!(
                "knot '{}' is not a gate",
                current.id
            )));
        }
        if current.state != workflow_runtime::EVALUATING {
            return Err(AppError::InvalidArgument(format!(
                "gate '{}' must be in '{}' to evaluate",
                current.id,
                workflow_runtime::EVALUATING
            )));
        }

        match decision {
            GateDecision::Yes => {
                let updated = self.write_state_change_locked(
                    &current,
                    "shipped",
                    false,
                    current.profile_etag.as_deref(),
                    &state_actor,
                    None,
                )?;
                Ok(GateEvaluationResult {
                    gate: self.apply_alias_to_knot(KnotView::from(updated))?,
                    decision: "yes".to_string(),
                    invariant: None,
                    reopened: Vec::new(),
                })
            }
            GateDecision::No => {
                let violated = non_empty(invariant.unwrap_or("")).ok_or_else(|| {
                    AppError::InvalidArgument(
                        "--invariant is required when gate decision is 'no'".to_string(),
                    )
                })?;
                let matches_invariant = current.invariants.iter().any(|item| {
                    crate::domain::gate::normalize_invariant_key(&item.condition)
                        == crate::domain::gate::normalize_invariant_key(&violated)
                });
                if !matches_invariant {
                    return Err(AppError::InvalidArgument(format!(
                        "gate '{}' does not define invariant '{}'",
                        current.id, violated
                    )));
                }
                let reopen_targets = current
                    .gate_data
                    .find_reopen_targets(&violated)
                    .cloned()
                    .ok_or_else(|| {
                        AppError::InvalidArgument(format!(
                            "gate '{}' has no failure mode for invariant '{}'",
                            current.id, violated
                        ))
                    })?;
                let mut reopened = Vec::new();
                for target in reopen_targets {
                    let target_id = self.resolve_knot_token(&target)?;
                    let target_record = db::get_knot_hot(&self.conn, &target_id)?
                        .ok_or_else(|| AppError::NotFound(target_id.clone()))?;
                    let reopened_record = if target_record.state == "ready_for_planning" {
                        target_record
                    } else {
                        self.write_state_change_locked(
                            &target_record,
                            "ready_for_planning",
                            true,
                            target_record.profile_etag.as_deref(),
                            &state_actor,
                            None,
                        )?
                    };
                    self.append_gate_failure_metadata_locked(
                        &reopened_record,
                        &current.id,
                        &violated,
                        &state_actor,
                    )?;
                    reopened.push(target_id);
                }
                let updated = self.write_state_change_locked(
                    &current,
                    "abandoned",
                    true,
                    current.profile_etag.as_deref(),
                    &state_actor,
                    None,
                )?;
                Ok(GateEvaluationResult {
                    gate: self.apply_alias_to_knot(KnotView::from(updated))?,
                    decision: "no".to_string(),
                    invariant: Some(violated),
                    reopened,
                })
            }
        }
    }

    pub fn pull(&self) -> Result<SyncSummary, AppError> {
        self.pull_with_progress(None)
    }

    pub fn pull_with_progress(
        &self,
        reporter: Option<&mut dyn ProgressReporter>,
    ) -> Result<SyncSummary, AppError> {
        let mut reporter = reporter;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        self.pull_unlocked_with_progress(&mut reporter)
    }

    pub fn pull_drift_warning(&self) -> Result<Option<PullDriftWarning>, AppError> {
        let threshold = self.read_pull_drift_warn_threshold()?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let service = ReplicationService::new(&self.conn, self.repo_root.clone());
        let unpushed_event_files = service.count_unpushed_event_files()?;
        if unpushed_event_files > threshold {
            Ok(Some(PullDriftWarning {
                unpushed_event_files,
                threshold,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn push(&self) -> Result<PushSummary, AppError> {
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let service = ReplicationService::new(&self.conn, self.repo_root.clone());
        Ok(service.push()?)
    }

    pub fn push_with_progress(
        &self,
        reporter: Option<&mut dyn ProgressReporter>,
    ) -> Result<PushSummary, AppError> {
        let mut reporter = reporter;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let service = ReplicationService::new(&self.conn, self.repo_root.clone());
        Ok(service.push_with_progress(&mut reporter)?)
    }

    pub fn sync(&self) -> Result<ReplicationSummary, AppError> {
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        let service = ReplicationService::new(&self.conn, self.repo_root.clone());
        Ok(service.sync()?)
    }

    pub fn sync_with_progress(
        &self,
        reporter: Option<&mut dyn ProgressReporter>,
    ) -> Result<ReplicationSummary, AppError> {
        let mut reporter = reporter;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        let service = ReplicationService::new(&self.conn, self.repo_root.clone());
        Ok(service.sync_with_progress(&mut reporter)?)
    }

    pub fn init_remote(&self) -> Result<(), AppError> {
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        crate::init::ensure_knots_gitignore(&self.repo_root)?;
        init_remote_knots_branch(&self.repo_root)?;
        Ok(())
    }

    pub fn fsck(&self) -> Result<FsckReport, AppError> {
        Ok(run_fsck(&self.repo_root)?)
    }

    pub fn doctor(&self, fix: bool) -> Result<DoctorReport, AppError> {
        Ok(run_doctor_with_fix(&self.repo_root, fix)?)
    }

    pub fn compact_write_snapshots(&self) -> Result<SnapshotWriteSummary, AppError> {
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        Ok(write_snapshots(&self.conn, &self.repo_root)?)
    }

    pub fn perf_harness(&self, iterations: u32) -> Result<PerfReport, AppError> {
        let _ = self;
        Ok(run_perf_harness(iterations)?)
    }

    pub fn add_edge(&self, src: &str, kind: &str, dst: &str) -> Result<EdgeView, AppError> {
        let src = self.resolve_knot_token(src)?;
        let dst = self.resolve_knot_token(dst)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        self.apply_edge_change(&src, kind, &dst, true)
    }

    pub fn remove_edge(&self, src: &str, kind: &str, dst: &str) -> Result<EdgeView, AppError> {
        let src = self.resolve_knot_token(src)?;
        let dst = self.resolve_knot_token(dst)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        self.apply_edge_change(&src, kind, &dst, false)
    }

    pub fn list_edges(&self, id: &str, direction: &str) -> Result<Vec<EdgeView>, AppError> {
        let id = self.resolve_knot_token(id)?;
        let direction = parse_edge_direction(direction)?;
        let rows = db::list_edges(&self.conn, &id, direction)?;
        Ok(rows.into_iter().map(EdgeView::from).collect())
    }

    pub fn list_layout_edges(&self) -> Result<Vec<EdgeView>, AppError> {
        let mut rows = db::list_edges_by_kind(&self.conn, "parent_of")?;
        rows.extend(db::list_edges_by_kind(&self.conn, "blocked_by")?);
        rows.extend(db::list_edges_by_kind(&self.conn, "blocks")?);
        Ok(rows.into_iter().map(EdgeView::from).collect())
    }

    pub fn cold_sync(&self) -> Result<SyncSummary, AppError> {
        self.pull()
    }

    pub fn cold_search(&self, term: &str) -> Result<Vec<ColdKnotView>, AppError> {
        self.maybe_auto_sync_for_read()?;
        Ok(db::search_cold_catalog(&self.conn, term)?
            .into_iter()
            .map(|record| ColdKnotView {
                id: record.id,
                title: record.title,
                state: record.state,
                updated_at: record.updated_at,
            })
            .collect())
    }

    pub fn rehydrate(&self, id: &str) -> Result<Option<KnotView>, AppError> {
        let id = self.resolve_knot_token(id)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        if let Some(knot) = db::get_knot_hot(&self.conn, &id)? {
            return Ok(Some(self.apply_alias_to_knot(KnotView::from(knot))?));
        }
        let warm = db::get_knot_warm(&self.conn, &id)?;
        let cold = db::get_cold_catalog(&self.conn, &id)?;
        let title = warm
            .as_ref()
            .map(|record| record.title.clone())
            .or_else(|| cold.as_ref().map(|record| record.title.clone()));
        let Some(title) = title else {
            return Ok(None);
        };
        let state = cold
            .as_ref()
            .map(|record| record.state.clone())
            .unwrap_or_else(|| "ready_for_implementation".to_string());
        let updated_at = cold
            .as_ref()
            .map(|record| record.updated_at.clone())
            .unwrap_or_else(now_utc_rfc3339);
        let record = rehydrate_from_events(&self.repo_root, &id, title, state, updated_at)?;
        db::upsert_knot_hot(
            &self.conn,
            &UpsertKnotHot {
                id: &id,
                title: &record.title,
                state: &record.state,
                updated_at: &record.updated_at,
                body: record.body.as_deref(),
                description: record.description.as_deref(),
                priority: record.priority,
                knot_type: Some(record.knot_type.as_str()),
                tags: &record.tags,
                notes: &record.notes,
                handoff_capsules: &record.handoff_capsules,
                invariants: &record.invariants,
                step_history: &record.step_history,
                gate_data: &record.gate_data,
                lease_data: &record.lease_data,
                lease_id: record.lease_id.as_deref(),
                profile_id: &record.profile_id,
                profile_etag: record.profile_etag.as_deref(),
                deferred_from_state: record.deferred_from_state.as_deref(),
                created_at: record.created_at.as_deref(),
            },
        )?;
        let hot =
            db::get_knot_hot(&self.conn, &id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
        Ok(Some(self.apply_alias_to_knot(KnotView::from(hot))?))
    }

    fn apply_edge_change(
        &self,
        src: &str,
        kind: &str,
        dst: &str,
        add: bool,
    ) -> Result<EdgeView, AppError> {
        if src.trim().is_empty() || kind.trim().is_empty() || dst.trim().is_empty() {
            return Err(AppError::InvalidArgument(
                "src, kind, and dst are required".to_string(),
            ));
        }

        let current = db::get_knot_hot(&self.conn, src)?
            .ok_or_else(|| AppError::NotFound(src.to_string()))?;
        let occurred_at = now_utc_rfc3339();
        let full_kind = if add {
            FullEventKind::KnotEdgeAdd
        } else {
            FullEventKind::KnotEdgeRemove
        };
        let full_event = FullEvent::with_identity(
            new_event_id(),
            occurred_at.clone(),
            src.to_string(),
            full_kind.as_str(),
            json!({
                "kind": kind,
                "dst": dst,
            }),
        );
        self.writer.write(&EventRecord::full(full_event))?;

        let profile = self.resolve_profile_for_record(&current)?;
        let profile_id = profile.id.clone();
        let knot_type = parse_knot_type(current.knot_type.as_deref());
        let terminal = workflow_runtime::is_terminal_state(
            &self.profile_registry,
            &profile_id,
            knot_type,
            &current.state,
        )?;
        let index_event_id = new_event_id();
        let idx_event = IndexEvent::with_identity(
            index_event_id.clone(),
            occurred_at.clone(),
            IndexEventKind::KnotHead.as_str(),
            build_knot_head_data(KnotHeadData {
                knot_id: src,
                title: &current.title,
                state: &current.state,
                profile_id: &profile_id,
                updated_at: &occurred_at,
                terminal,
                deferred_from_state: current.deferred_from_state.as_deref(),
                invariants: &current.invariants,
                knot_type,
                gate_data: &current.gate_data,
            }),
        );
        self.writer.write(&EventRecord::index(idx_event))?;

        if add {
            db::insert_edge(&self.conn, src, kind, dst)?;
        } else {
            db::delete_edge(&self.conn, src, kind, dst)?;
        }

        db::upsert_knot_hot(
            &self.conn,
            &UpsertKnotHot {
                id: src,
                title: &current.title,
                state: &current.state,
                updated_at: &occurred_at,
                body: current.body.as_deref(),
                description: current.description.as_deref(),
                priority: current.priority,
                knot_type: current.knot_type.as_deref(),
                tags: &current.tags,
                notes: &current.notes,
                handoff_capsules: &current.handoff_capsules,
                invariants: &current.invariants,
                step_history: &current.step_history,
                gate_data: &current.gate_data,
                lease_data: &current.lease_data,
                lease_id: current.lease_id.as_deref(),
                profile_id: &profile_id,
                profile_etag: Some(&index_event_id),
                deferred_from_state: current.deferred_from_state.as_deref(),
                created_at: current.created_at.as_deref(),
            },
        )?;
        Ok(EdgeView {
            src: src.to_string(),
            kind: kind.to_string(),
            dst: dst.to_string(),
        })
    }
}

fn ensure_parent_dir(path: &str) -> Result<(), AppError> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn parse_edge_direction(raw: &str) -> Result<EdgeDirection, AppError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "incoming" | "in" => Ok(EdgeDirection::Incoming),
        "outgoing" | "out" => Ok(EdgeDirection::Outgoing),
        "both" | "all" => Ok(EdgeDirection::Both),
        _ => Err(AppError::InvalidArgument(format!(
            "unsupported edge direction '{}'; use incoming|outgoing|both",
            raw
        ))),
    }
}

fn non_empty(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn canonical_profile_id(raw: &str) -> String {
    normalize_profile_id(raw).unwrap_or_else(|| raw.trim().to_ascii_lowercase())
}

fn normalize_state_input(raw: &str) -> Result<String, AppError> {
    let parsed = crate::domain::state::KnotState::from_str(raw)?;
    Ok(parsed.as_str().to_string())
}

fn normalize_tag(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

fn next_deferred_from_state(current: &KnotCacheRecord, next_state: &str) -> Option<String> {
    if next_state == "deferred" && current.state != "deferred" {
        Some(current.state.clone())
    } else if current.state == "deferred" && next_state != "deferred" {
        None
    } else {
        current.deferred_from_state.clone()
    }
}

fn require_state_for_knot_type(
    knot_type: KnotType,
    profile: &ProfileDefinition,
    state: &str,
) -> Result<(), AppError> {
    match knot_type {
        KnotType::Work => Ok(profile.require_state(state)?),
        KnotType::Gate => {
            if matches!(
                state,
                workflow_runtime::READY_TO_EVALUATE
                    | workflow_runtime::EVALUATING
                    | "shipped"
                    | "abandoned"
            ) {
                Ok(())
            } else {
                Err(AppError::InvalidArgument(format!(
                    "state '{}' is not valid for gate knots",
                    state
                )))
            }
        }
        KnotType::Lease => {
            if matches!(
                state,
                workflow_runtime::LEASE_READY
                    | workflow_runtime::LEASE_ACTIVE
                    | workflow_runtime::LEASE_TERMINATED
            ) {
                Ok(())
            } else {
                Err(AppError::InvalidArgument(format!(
                    "state '{}' is not valid for lease knots",
                    state
                )))
            }
        }
    }
}

fn require_gate_metadata_scope(knot_type: KnotType) -> Result<(), AppError> {
    if knot_type == KnotType::Gate {
        Ok(())
    } else {
        Err(AppError::InvalidArgument(
            "gate owner/failure mode fields require knot type 'gate'".to_string(),
        ))
    }
}

struct KnotHeadData<'a> {
    knot_id: &'a str,
    title: &'a str,
    state: &'a str,
    profile_id: &'a str,
    updated_at: &'a str,
    terminal: bool,
    deferred_from_state: Option<&'a str>,
    invariants: &'a [Invariant],
    knot_type: KnotType,
    gate_data: &'a GateData,
}

fn build_knot_head_data(head: KnotHeadData<'_>) -> Value {
    let mut payload = serde_json::Map::new();
    payload.insert(
        "knot_id".to_string(),
        Value::String(head.knot_id.to_string()),
    );
    payload.insert("title".to_string(), Value::String(head.title.to_string()));
    payload.insert("state".to_string(), Value::String(head.state.to_string()));
    payload.insert(
        "profile_id".to_string(),
        Value::String(head.profile_id.to_string()),
    );
    payload.insert(
        "updated_at".to_string(),
        Value::String(head.updated_at.to_string()),
    );
    payload.insert("terminal".to_string(), Value::Bool(head.terminal));
    payload.insert(
        "type".to_string(),
        Value::String(head.knot_type.as_str().to_string()),
    );
    payload.insert(
        "invariants".to_string(),
        serde_json::to_value(head.invariants).expect("invariants should serialize"),
    );
    payload.insert(
        "gate".to_string(),
        serde_json::to_value(head.gate_data).expect("gate data should serialize"),
    );
    if let Some(value) = head.deferred_from_state {
        payload.insert(
            "deferred_from_state".to_string(),
            Value::String(value.to_string()),
        );
    } else {
        payload.insert("deferred_from_state".to_string(), Value::Null);
    }
    Value::Object(payload)
}

fn build_state_event_data(
    from: &str,
    to: &str,
    profile_id: &str,
    force: bool,
    deferred_from_state: Option<&str>,
    state_actor: &StateActorMetadata,
    cascade: Option<StateCascadeMetadata<'_>>,
) -> Result<Value, AppError> {
    let mut payload = serde_json::Map::new();
    payload.insert("from".to_string(), Value::String(from.to_string()));
    payload.insert("to".to_string(), Value::String(to.to_string()));
    payload.insert(
        "profile_id".to_string(),
        Value::String(profile_id.to_string()),
    );
    payload.insert("force".to_string(), Value::Bool(force));
    if let Some(value) = deferred_from_state {
        payload.insert(
            "deferred_from_state".to_string(),
            Value::String(value.to_string()),
        );
    }
    if let Some(cascade) = cascade {
        payload.insert("cascade_approved".to_string(), Value::Bool(true));
        payload.insert(
            "cascade_root_id".to_string(),
            Value::String(cascade.root_id.to_string()),
        );
    }
    append_state_actor_metadata(&mut payload, state_actor)?;
    Ok(Value::Object(payload))
}

fn append_state_actor_metadata(
    payload: &mut serde_json::Map<String, Value>,
    state_actor: &StateActorMetadata,
) -> Result<(), AppError> {
    if let Some(raw_kind) = state_actor.actor_kind.as_deref().and_then(non_empty) {
        let kind = raw_kind.to_ascii_lowercase();
        if kind != "human" && kind != "agent" {
            return Err(AppError::InvalidArgument(
                "--actor-kind must be one of: human, agent".to_string(),
            ));
        }
        payload.insert("actor_kind".to_string(), Value::String(kind));
    }

    if let Some(agent_name) = state_actor.agent_name.as_deref().and_then(non_empty) {
        payload.insert("agent_name".to_string(), Value::String(agent_name));
    }
    if let Some(agent_model) = state_actor.agent_model.as_deref().and_then(non_empty) {
        payload.insert("agent_model".to_string(), Value::String(agent_model));
    }
    if let Some(agent_version) = state_actor.agent_version.as_deref().and_then(non_empty) {
        payload.insert("agent_version".to_string(), Value::String(agent_version));
    }

    Ok(())
}

fn metadata_entry_from_input(
    input: MetadataEntryInput,
    fallback_datetime: &str,
) -> Result<MetadataEntry, AppError> {
    if input.content.trim().is_empty() {
        return Err(AppError::InvalidArgument(
            "metadata content cannot be empty".to_string(),
        ));
    }
    if let Some(raw) = input.datetime.as_deref() {
        if normalize_datetime(Some(raw)).is_none() {
            return Err(AppError::InvalidArgument(
                "metadata datetime must be RFC3339".to_string(),
            ));
        }
    }
    Ok(MetadataEntry::from_input(input, fallback_datetime))
}

fn ensure_profile_etag(
    current: &KnotCacheRecord,
    expected_profile_etag: Option<&str>,
) -> Result<(), AppError> {
    let Some(expected) = expected_profile_etag else {
        return Ok(());
    };
    let current_etag = current.profile_etag.as_deref().unwrap_or("");
    if current_etag == expected {
        return Ok(());
    }
    Err(AppError::StaleWorkflowHead {
        expected: expected.to_string(),
        current: current_etag.to_string(),
    })
}

fn is_action_state(state: &str) -> bool {
    workflow_runtime::is_action_state(state)
}

fn apply_step_transition(
    existing: &[StepRecord],
    from_state: &str,
    to_state: &str,
    occurred_at: &str,
    actor: &StateActorMetadata,
) -> Vec<StepRecord> {
    let mut history: Vec<StepRecord> = existing.to_vec();
    if from_state != to_state {
        for record in &mut history {
            if record.is_active() {
                record.to_state = Some(to_state.to_string());
                record.ended_at = Some(occurred_at.to_string());
                record.status = StepStatus::Completed;
            }
        }
    }
    if is_action_state(to_state) && from_state != to_state {
        let step_actor = StepActorInfo {
            actor_kind: actor.actor_kind.clone(),
            agent_name: actor.agent_name.clone(),
            agent_model: actor.agent_model.clone(),
            agent_version: actor.agent_version.clone(),
            ..Default::default()
        };
        let phase = derive_phase(to_state);
        let record = StepRecord::new_started(to_state, phase, from_state, occurred_at, &step_actor);
        history.push(record);
    }
    history
}

pub fn annotate_step_history(
    existing: &[StepRecord],
    actor: &StepActorInfo,
    occurred_at: &str,
) -> Vec<StepRecord> {
    let mut history: Vec<StepRecord> = existing.to_vec();
    let has_active = history.iter().any(|r| r.is_active());
    if has_active {
        let mut new_record: Option<StepRecord> = None;
        for record in &mut history {
            if record.is_active() {
                record.ended_at = Some(occurred_at.to_string());
                record.status = StepStatus::Completed;
                new_record = Some(StepRecord::new_started(
                    &record.step,
                    &record.phase,
                    &record.from_state,
                    occurred_at,
                    actor,
                ));
            }
        }
        if let Some(new) = new_record {
            history.push(new);
        }
    }
    history
}

struct RehydrateProjection {
    title: String,
    state: String,
    updated_at: String,
    body: Option<String>,
    description: Option<String>,
    priority: Option<i64>,
    knot_type: KnotType,
    tags: Vec<String>,
    notes: Vec<MetadataEntry>,
    handoff_capsules: Vec<MetadataEntry>,
    invariants: Vec<Invariant>,
    step_history: Vec<StepRecord>,
    gate_data: GateData,
    lease_data: LeaseData,
    lease_id: Option<String>,
    profile_id: String,
    profile_etag: Option<String>,
    deferred_from_state: Option<String>,
    created_at: Option<String>,
}

fn rehydrate_from_events(
    repo_root: &std::path::Path,
    knot_id: &str,
    title: String,
    state: String,
    updated_at: String,
) -> Result<RehydrateProjection, AppError> {
    let mut projection = RehydrateProjection {
        title,
        state,
        updated_at: updated_at.clone(),
        body: None,
        description: None,
        priority: None,
        knot_type: KnotType::default(),
        tags: Vec::new(),
        notes: Vec::new(),
        handoff_capsules: Vec::new(),
        invariants: Vec::new(),
        step_history: Vec::new(),
        gate_data: GateData::default(),
        lease_data: LeaseData::default(),
        lease_id: None,
        profile_id: String::new(),
        profile_etag: None,
        deferred_from_state: None,
        created_at: Some(updated_at),
    };

    let mut stack = vec![repo_root.join(".knots").join("events")];
    let mut full_paths = Vec::new();
    while let Some(dir) = stack.pop() {
        if !dir.exists() {
            continue;
        }
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "json") {
                full_paths.push(path);
            }
        }
    }
    full_paths.sort();

    for path in full_paths {
        let bytes = fs::read(&path)?;
        let event: FullEvent = serde_json::from_slice(&bytes).map_err(|err| {
            AppError::InvalidArgument(format!(
                "invalid rehydrate event '{}': {}",
                path.display(),
                err
            ))
        })?;
        if event.knot_id != knot_id {
            continue;
        }
        apply_rehydrate_event(&mut projection, &event);
    }

    let mut idx_stack = vec![repo_root.join(".knots").join("index")];
    let mut idx_paths = Vec::new();
    while let Some(dir) = idx_stack.pop() {
        if !dir.exists() {
            continue;
        }
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.is_dir() {
                idx_stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "json") {
                idx_paths.push(path);
            }
        }
    }
    idx_paths.sort();
    for path in idx_paths {
        let bytes = fs::read(&path)?;
        let event: IndexEvent = serde_json::from_slice(&bytes).map_err(|err| {
            AppError::InvalidArgument(format!(
                "invalid rehydrate index '{}': {}",
                path.display(),
                err
            ))
        })?;
        if event.event_type != IndexEventKind::KnotHead.as_str() {
            continue;
        }
        let Some(data) = event.data.as_object() else {
            continue;
        };
        if data.get("knot_id").and_then(Value::as_str) != Some(knot_id) {
            continue;
        }
        if let Some(title) = data.get("title").and_then(Value::as_str) {
            projection.title = title.to_string();
        }
        if let Some(state) = data.get("state").and_then(Value::as_str) {
            projection.state = state.to_string();
        }
        if let Some(updated_at) = data.get("updated_at").and_then(Value::as_str) {
            projection.updated_at = updated_at.to_string();
        }
        if let Some(raw_type) = data.get("type").and_then(Value::as_str) {
            projection.knot_type = parse_knot_type(Some(raw_type));
        }
        let raw_profile_id = data
            .get("profile_id")
            .and_then(Value::as_str)
            .or_else(|| data.get("workflow_id").and_then(Value::as_str));
        if let Some(raw_profile_id) = raw_profile_id {
            if let Some(profile_id) = normalize_profile_id(raw_profile_id) {
                projection.profile_id = profile_id;
            }
        }
        projection.deferred_from_state = data
            .get("deferred_from_state")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        if data.contains_key("invariants") {
            projection.invariants = parse_invariants_value(data.get("invariants"));
        }
        if data.contains_key("gate") {
            projection.gate_data = parse_gate_data_value(data.get("gate"));
        }
        projection.profile_etag = Some(event.event_id.clone());
    }

    if projection.profile_id.trim().is_empty() {
        return Err(AppError::InvalidArgument(format!(
            "rehydrate events for '{}' are missing profile_id",
            knot_id
        )));
    }

    Ok(projection)
}

fn apply_rehydrate_event(projection: &mut RehydrateProjection, event: &FullEvent) {
    let Some(data) = event.data.as_object() else {
        return;
    };

    match event.event_type.as_str() {
        "knot.created" => {
            if let Some(title) = data.get("title").and_then(Value::as_str) {
                projection.title = title.to_string();
            }
            if let Some(state) = data.get("state").and_then(Value::as_str) {
                projection.state = state.to_string();
            }
            let raw_profile_id = data
                .get("profile_id")
                .and_then(Value::as_str)
                .or_else(|| data.get("workflow_id").and_then(Value::as_str));
            if let Some(raw_profile_id) = raw_profile_id {
                if let Some(profile_id) = normalize_profile_id(raw_profile_id) {
                    projection.profile_id = profile_id;
                }
            }
            projection.deferred_from_state = data
                .get("deferred_from_state")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            if data.contains_key("invariants") {
                projection.invariants = parse_invariants_value(data.get("invariants"));
            }
            if let Some(raw_type) = data.get("type").and_then(Value::as_str) {
                projection.knot_type = parse_knot_type(Some(raw_type));
            }
            if data.contains_key("gate") {
                projection.gate_data = parse_gate_data_value(data.get("gate"));
            }
            projection.created_at = Some(event.occurred_at.clone());
            projection.updated_at = event.occurred_at.clone();
        }
        "knot.title_set" => {
            if let Some(value) = data.get("to").and_then(Value::as_str) {
                projection.title = value.to_string();
                projection.updated_at = event.occurred_at.clone();
            }
        }
        "knot.state_set" => {
            if let Some(value) = data.get("to").and_then(Value::as_str) {
                projection.state = value.to_string();
                projection.updated_at = event.occurred_at.clone();
            }
            projection.deferred_from_state = data
                .get("deferred_from_state")
                .and_then(Value::as_str)
                .map(ToString::to_string);
        }
        "knot.profile_set" => {
            let raw_profile_id = data
                .get("to_profile_id")
                .and_then(Value::as_str)
                .or_else(|| data.get("profile_id").and_then(Value::as_str))
                .or_else(|| data.get("workflow_id").and_then(Value::as_str));
            if let Some(raw_profile_id) = raw_profile_id {
                if let Some(profile_id) = normalize_profile_id(raw_profile_id) {
                    projection.profile_id = profile_id;
                }
            }
            if let Some(state) = data.get("to_state").and_then(Value::as_str) {
                projection.state = state.to_string();
            }
            projection.deferred_from_state = data
                .get("deferred_from_state")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            projection.updated_at = event.occurred_at.clone();
        }
        "knot.description_set" => {
            let next = data
                .get("description")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string);
            projection.description = next.clone();
            projection.body = next;
            projection.updated_at = event.occurred_at.clone();
        }
        "knot.priority_set" => {
            projection.priority = data.get("priority").and_then(Value::as_i64);
            projection.updated_at = event.occurred_at.clone();
        }
        "knot.type_set" => {
            let raw = data.get("type").and_then(Value::as_str);
            projection.knot_type = parse_knot_type(raw);
            projection.updated_at = event.occurred_at.clone();
        }
        "knot.gate_data_set" => {
            projection.gate_data = parse_gate_data_value(data.get("gate"));
            projection.updated_at = event.occurred_at.clone();
        }
        "knot.tag_add" => {
            if let Some(tag) = data.get("tag").and_then(Value::as_str) {
                let normalized = tag.trim().to_ascii_lowercase();
                if !normalized.is_empty()
                    && !projection
                        .tags
                        .iter()
                        .any(|existing| existing == &normalized)
                {
                    projection.tags.push(normalized);
                }
            }
        }
        "knot.tag_remove" => {
            if let Some(tag) = data.get("tag").and_then(Value::as_str) {
                let normalized = tag.trim().to_ascii_lowercase();
                projection.tags.retain(|existing| existing != &normalized);
            }
        }
        "knot.note_added" => {
            if let Some(entry) = parse_metadata_entry_for_rehydrate(data) {
                if !projection
                    .notes
                    .iter()
                    .any(|existing| existing.entry_id == entry.entry_id)
                {
                    projection.notes.push(entry);
                }
            }
        }
        "knot.handoff_capsule_added" => {
            if let Some(entry) = parse_metadata_entry_for_rehydrate(data) {
                if !projection
                    .handoff_capsules
                    .iter()
                    .any(|existing| existing.entry_id == entry.entry_id)
                {
                    projection.handoff_capsules.push(entry);
                }
            }
        }
        "knot.invariants_set" => {
            projection.invariants = parse_invariants_value(data.get("invariants"));
            projection.updated_at = event.occurred_at.clone();
        }
        _ => {}
    }
}

fn parse_metadata_entry_for_rehydrate(
    data: &serde_json::Map<String, Value>,
) -> Option<MetadataEntry> {
    let entry_id = data.get("entry_id")?.as_str()?.to_string();
    let content = data.get("content")?.as_str()?.to_string();
    let username = data.get("username")?.as_str()?.to_string();
    let datetime = data.get("datetime")?.as_str()?.to_string();
    let agentname = data.get("agentname")?.as_str()?.to_string();
    let model = data.get("model")?.as_str()?.to_string();
    let version = data.get("version")?.as_str()?.to_string();
    Some(MetadataEntry {
        entry_id,
        content,
        username,
        datetime,
        agentname,
        model,
        version,
    })
}

fn parse_invariants_value(value: Option<&Value>) -> Vec<Invariant> {
    let Some(value) = value.cloned() else {
        return Vec::new();
    };
    serde_json::from_value(value).unwrap_or_default()
}

fn parse_gate_data_value(value: Option<&Value>) -> GateData {
    let Some(value) = value.cloned() else {
        return GateData::default();
    };
    serde_json::from_value(value).unwrap_or_default()
}

impl From<KnotCacheRecord> for KnotView {
    fn from(value: KnotCacheRecord) -> Self {
        let profile_id = canonical_profile_id(&value.profile_id);
        let knot_type = parse_knot_type(value.knot_type.as_deref());
        let gate = (knot_type == KnotType::Gate).then_some(value.gate_data.clone());
        let lease = (knot_type == KnotType::Lease).then_some(value.lease_data.clone());
        Self {
            id: value.id,
            alias: None,
            title: value.title,
            state: value.state,
            updated_at: value.updated_at,
            body: value.body,
            description: value.description,
            priority: value.priority,
            knot_type,
            tags: value.tags,
            notes: value.notes,
            handoff_capsules: value.handoff_capsules,
            invariants: value.invariants,
            step_history: value.step_history,
            gate,
            lease,
            lease_id: value.lease_id,
            profile_id,
            profile_etag: value.profile_etag,
            deferred_from_state: value.deferred_from_state,
            created_at: value.created_at,
            edges: Vec::new(),
        }
    }
}

impl From<EdgeRecord> for EdgeView {
    fn from(value: EdgeRecord) -> Self {
        Self {
            src: value.src,
            kind: value.kind,
            dst: value.dst,
        }
    }
}

#[derive(Debug)]
pub enum AppError {
    Io(std::io::Error),
    Db(rusqlite::Error),
    Event(EventWriteError),
    Sync(SyncError),
    Lock(LockError),
    RemoteInit(RemoteInitError),
    Fsck(FsckError),
    Doctor(DoctorError),
    Snapshot(SnapshotError),
    Perf(PerfError),
    Workflow(ProfileError),
    ParseState(ParseKnotStateError),
    InvalidTransition(InvalidStateTransition),
    StaleWorkflowHead {
        expected: String,
        current: String,
    },
    HierarchyProgressBlocked {
        knot_id: String,
        target_state: String,
        blockers: Vec<HierarchyKnot>,
    },
    TerminalCascadeApprovalRequired {
        knot_id: String,
        target_state: String,
        descendants: Vec<HierarchyKnot>,
    },
    InvalidArgument(String),
    NotFound(String),
    NotInitialized,
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Io(err) => write!(f, "I/O error: {}", err),
            AppError::Db(err) => write!(f, "database error: {}", err),
            AppError::Event(err) => write!(f, "event write error: {}", err),
            AppError::Sync(err) => write!(f, "sync error: {}", err),
            AppError::Lock(err) => write!(f, "lock error: {}", err),
            AppError::RemoteInit(err) => write!(f, "remote init error: {}", err),
            AppError::Fsck(err) => write!(f, "fsck error: {}", err),
            AppError::Doctor(err) => write!(f, "doctor error: {}", err),
            AppError::Snapshot(err) => write!(f, "snapshot error: {}", err),
            AppError::Perf(err) => write!(f, "perf error: {}", err),
            AppError::Workflow(err) => write!(f, "workflow error: {}", err),
            AppError::ParseState(err) => write!(f, "state parse error: {}", err),
            AppError::InvalidTransition(err) => write!(f, "{}", err),
            AppError::StaleWorkflowHead { expected, current } => write!(
                f,
                "stale profile_etag: expected '{}', current '{}'",
                expected, current
            ),
            AppError::HierarchyProgressBlocked {
                knot_id,
                target_state,
                blockers,
            } => write!(
                f,
                "{}: cannot move '{}' to '{}' because direct child knots are behind; blockers: {}",
                state_hierarchy::HIERARCHY_PROGRESS_BLOCKED_CODE,
                knot_id,
                target_state,
                state_hierarchy::format_hierarchy_knots(blockers)
            ),
            AppError::TerminalCascadeApprovalRequired {
                knot_id,
                target_state,
                descendants,
            } => write!(
                f,
                "{}: moving '{}' to '{}' requires approval because all descendants will also \
                 move to that terminal state; descendants: {}; rerun with \
                 --cascade-terminal-descendants or approve the interactive prompt",
                state_hierarchy::TERMINAL_CASCADE_APPROVAL_REQUIRED_CODE,
                knot_id,
                target_state,
                state_hierarchy::format_hierarchy_knots(descendants)
            ),
            AppError::InvalidArgument(message) => write!(f, "{}", message),
            AppError::NotFound(id) => write!(f, "knot '{}' not found in local cache", id),
            AppError::NotInitialized => write!(
                f,
                "knots is not initialized in this repository; run `kno init` first"
            ),
        }
    }
}

impl Error for AppError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            AppError::Io(err) => Some(err),
            AppError::Db(err) => Some(err),
            AppError::Event(err) => Some(err),
            AppError::Sync(err) => Some(err),
            AppError::Lock(err) => Some(err),
            AppError::RemoteInit(err) => Some(err),
            AppError::Fsck(err) => Some(err),
            AppError::Doctor(err) => Some(err),
            AppError::Snapshot(err) => Some(err),
            AppError::Perf(err) => Some(err),
            AppError::Workflow(err) => Some(err),
            AppError::ParseState(err) => Some(err),
            AppError::InvalidTransition(err) => Some(err),
            AppError::StaleWorkflowHead { .. } => None,
            AppError::HierarchyProgressBlocked { .. } => None,
            AppError::TerminalCascadeApprovalRequired { .. } => None,
            AppError::InvalidArgument(_) => None,
            AppError::NotFound(_) => None,
            AppError::NotInitialized => None,
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        AppError::Io(value)
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(value: rusqlite::Error) -> Self {
        AppError::Db(value)
    }
}

impl From<EventWriteError> for AppError {
    fn from(value: EventWriteError) -> Self {
        AppError::Event(value)
    }
}

impl From<SyncError> for AppError {
    fn from(value: SyncError) -> Self {
        AppError::Sync(value)
    }
}

impl From<LockError> for AppError {
    fn from(value: LockError) -> Self {
        AppError::Lock(value)
    }
}

impl From<RemoteInitError> for AppError {
    fn from(value: RemoteInitError) -> Self {
        AppError::RemoteInit(value)
    }
}

impl From<FsckError> for AppError {
    fn from(value: FsckError) -> Self {
        AppError::Fsck(value)
    }
}

impl From<DoctorError> for AppError {
    fn from(value: DoctorError) -> Self {
        AppError::Doctor(value)
    }
}

impl From<SnapshotError> for AppError {
    fn from(value: SnapshotError) -> Self {
        AppError::Snapshot(value)
    }
}

impl From<PerfError> for AppError {
    fn from(value: PerfError) -> Self {
        AppError::Perf(value)
    }
}

impl From<ProfileError> for AppError {
    fn from(value: ProfileError) -> Self {
        AppError::Workflow(value)
    }
}

impl From<ParseKnotStateError> for AppError {
    fn from(value: ParseKnotStateError) -> Self {
        AppError::ParseState(value)
    }
}

impl From<InvalidStateTransition> for AppError {
    fn from(value: InvalidStateTransition) -> Self {
        AppError::InvalidTransition(value)
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
#[path = "app/tests_coverage_ext.rs"]
mod tests_coverage_ext;
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
#[path = "app/tests_step_history.rs"]
mod tests_step_history;
#[cfg(test)]
#[path = "app/tests_terminal_deferred.rs"]
mod tests_terminal_deferred;
