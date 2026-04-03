use std::error::Error;
use std::fmt;

use crate::doctor::DoctorError;
use crate::domain::state::{InvalidStateTransition, ParseKnotStateError};
use crate::events::EventWriteError;
use crate::fsck::FsckError;
use crate::locks::LockError;
use crate::perf::PerfError;
use crate::remote_init::RemoteInitError;
use crate::snapshots::SnapshotError;
use crate::state_hierarchy::{self, HierarchyKnot};
use crate::sync::SyncError;
use crate::workflow::ProfileError;

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
    ImmutableRecordViolation(String),
    InvalidArgument(String),
    UnsupportedDistribution {
        action: String,
        mode: String,
    },
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
            AppError::RemoteInit(err) => {
                write!(f, "remote init error: {}", err)
            }
            AppError::Fsck(err) => write!(f, "fsck error: {}", err),
            AppError::Doctor(err) => write!(f, "doctor error: {}", err),
            AppError::Snapshot(err) => write!(f, "snapshot error: {}", err),
            AppError::Perf(err) => write!(f, "perf error: {}", err),
            AppError::Workflow(err) => write!(f, "workflow error: {}", err),
            AppError::ParseState(err) => {
                write!(f, "state parse error: {}", err)
            }
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
                "{}: cannot move '{}' to '{}' because direct child knots \
                 are behind; blockers: {}",
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
                "{}: moving '{}' to '{}' requires approval because all \
                 descendants will also move to that terminal state; \
                 descendants: {}; rerun with \
                 --cascade-terminal-descendants or approve the \
                 interactive prompt",
                state_hierarchy::TERMINAL_CASCADE_APPROVAL_REQUIRED_CODE,
                knot_id,
                target_state,
                state_hierarchy::format_hierarchy_knots(descendants)
            ),
            AppError::ImmutableRecordViolation(message) => write!(f, "{}", message),
            AppError::InvalidArgument(message) => write!(f, "{}", message),
            AppError::UnsupportedDistribution { action, mode } => {
                write!(f, "{action} is not supported in {mode} mode")
            }
            AppError::NotFound(id) => {
                write!(f, "knot '{}' not found in local cache", id)
            }
            AppError::NotInitialized => write!(
                f,
                "knots is not initialized in this repository; \
                 run `kno init` first"
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
            AppError::StaleWorkflowHead { .. }
            | AppError::HierarchyProgressBlocked { .. }
            | AppError::TerminalCascadeApprovalRequired { .. }
            | AppError::ImmutableRecordViolation(_)
            | AppError::InvalidArgument(_)
            | AppError::UnsupportedDistribution { .. }
            | AppError::NotFound(_)
            | AppError::NotInitialized => None,
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
