use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::locks::{FileLock, LockError};

const QUEUE_DIR: &str = ".knots/queue";
const REQUESTS_DIR: &str = "writes";
const RESPONSES_DIR: &str = "responses";
const WORKER_LOCK_FILE: &str = ".knots/locks/write_queue_worker.lock";
const WAIT_TIMEOUT: Duration = Duration::from_secs(120);
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(20);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewOperation {
    pub title: String,
    pub description: Option<String>,
    pub state: Option<String>,
    pub profile: Option<String>,
    pub fast: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuickNewOperation {
    pub title: String,
    pub description: Option<String>,
    pub state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateOperation {
    pub id: String,
    pub state: String,
    pub force: bool,
    pub if_match: Option<String>,
    pub actor_kind: Option<String>,
    pub agent_name: Option<String>,
    pub agent_model: Option<String>,
    pub agent_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateOperation {
    pub id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub priority: Option<i64>,
    pub status: Option<String>,
    pub knot_type: Option<String>,
    pub add_tags: Vec<String>,
    pub remove_tags: Vec<String>,
    pub add_invariants: Vec<String>,
    pub remove_invariants: Vec<String>,
    pub clear_invariants: bool,
    pub add_note: Option<String>,
    pub note_username: Option<String>,
    pub note_datetime: Option<String>,
    pub note_agentname: Option<String>,
    pub note_model: Option<String>,
    pub note_version: Option<String>,
    pub add_handoff_capsule: Option<String>,
    pub handoff_username: Option<String>,
    pub handoff_datetime: Option<String>,
    pub handoff_agentname: Option<String>,
    pub handoff_model: Option<String>,
    pub handoff_version: Option<String>,
    pub if_match: Option<String>,
    pub actor_kind: Option<String>,
    pub agent_name: Option<String>,
    pub agent_model: Option<String>,
    pub agent_version: Option<String>,
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NextOperation {
    pub id: String,
    pub expected_state: Option<String>,
    pub json: bool,
    pub actor_kind: Option<String>,
    pub agent_name: Option<String>,
    pub agent_model: Option<String>,
    pub agent_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClaimOperation {
    pub id: String,
    pub json: bool,
    pub agent_name: Option<String>,
    pub agent_model: Option<String>,
    pub agent_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PollClaimOperation {
    pub stage: Option<String>,
    pub owner: Option<String>,
    pub json: bool,
    pub agent_name: Option<String>,
    pub agent_model: Option<String>,
    pub agent_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EdgeOperation {
    pub src: String,
    pub kind: String,
    pub dst: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum WriteOperation {
    New(NewOperation),
    QuickNew(QuickNewOperation),
    State(StateOperation),
    Update(UpdateOperation),
    Next(NextOperation),
    Claim(ClaimOperation),
    PollClaim(PollClaimOperation),
    EdgeAdd(EdgeOperation),
    EdgeRemove(EdgeOperation),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueuedWriteRequest {
    pub request_id: String,
    pub repo_root: String,
    pub db_path: String,
    pub response_path: String,
    pub operation: WriteOperation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueuedWriteResponse {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

impl QueuedWriteResponse {
    pub fn success(output: String) -> Self {
        Self {
            success: true,
            output,
            error: None,
        }
    }

    pub fn failure(error: String) -> Self {
        Self {
            success: false,
            output: String::new(),
            error: Some(error),
        }
    }
}

#[derive(Debug)]
pub enum QueueError {
    Io(std::io::Error),
    Serde(serde_json::Error),
    Lock(LockError),
    Timeout(Duration),
}

impl std::fmt::Display for QueueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QueueError::Io(err) => write!(f, "queue I/O error: {}", err),
            QueueError::Serde(err) => write!(f, "queue serialization error: {}", err),
            QueueError::Lock(err) => write!(f, "queue lock error: {}", err),
            QueueError::Timeout(timeout) => {
                write!(
                    f,
                    "timed out waiting for queued write response after {:?}",
                    timeout
                )
            }
        }
    }
}

impl std::error::Error for QueueError {}

impl From<std::io::Error> for QueueError {
    fn from(value: std::io::Error) -> Self {
        QueueError::Io(value)
    }
}

impl From<serde_json::Error> for QueueError {
    fn from(value: serde_json::Error) -> Self {
        QueueError::Serde(value)
    }
}

impl From<LockError> for QueueError {
    fn from(value: LockError) -> Self {
        QueueError::Lock(value)
    }
}

#[derive(Debug, Clone)]
struct QueuePaths {
    requests_dir: PathBuf,
    responses_dir: PathBuf,
    worker_lock_path: PathBuf,
}

impl QueuePaths {
    fn for_repo(repo_root: &Path) -> Self {
        let queue_dir = repo_root.join(QUEUE_DIR);
        Self {
            requests_dir: queue_dir.join(REQUESTS_DIR),
            responses_dir: queue_dir.join(RESPONSES_DIR),
            worker_lock_path: repo_root.join(WORKER_LOCK_FILE),
        }
    }

    fn create_dirs(&self) -> Result<(), QueueError> {
        fs::create_dir_all(&self.requests_dir)?;
        fs::create_dir_all(&self.responses_dir)?;
        Ok(())
    }
}

pub fn enqueue_and_wait<F>(
    repo_root: &Path,
    db_path: &str,
    operation: WriteOperation,
    mut executor: F,
) -> Result<QueuedWriteResponse, QueueError>
where
    F: FnMut(&QueuedWriteRequest) -> QueuedWriteResponse,
{
    let absolute_root = canonical_or_original(repo_root);
    let absolute_db = to_absolute_db_path(&absolute_root, db_path);
    let paths = QueuePaths::for_repo(&absolute_root);
    paths.create_dirs()?;

    let request_id = uuid::Uuid::now_v7().to_string();
    let response_path = paths
        .responses_dir
        .join(format!("{}.json", request_id))
        .display()
        .to_string();
    let request = QueuedWriteRequest {
        request_id: request_id.clone(),
        repo_root: absolute_root.display().to_string(),
        db_path: absolute_db,
        response_path: response_path.clone(),
        operation,
    };
    enqueue_request(&paths, &request)?;

    let start = std::time::Instant::now();
    loop {
        drain_pending_requests(&paths, &mut executor)?;
        if let Some(response) = read_response_file(Path::new(&response_path))? {
            let _ = fs::remove_file(&response_path);
            return Ok(response);
        }
        if start.elapsed() >= WAIT_TIMEOUT {
            return Err(QueueError::Timeout(WAIT_TIMEOUT));
        }
        thread::sleep(WAIT_POLL_INTERVAL);
    }
}

fn canonical_or_original(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn to_absolute_db_path(repo_root: &Path, db_path: &str) -> String {
    let db = Path::new(db_path);
    if db.is_absolute() {
        db.display().to_string()
    } else {
        repo_root.join(db).display().to_string()
    }
}

fn enqueue_request(paths: &QueuePaths, request: &QueuedWriteRequest) -> Result<(), QueueError> {
    let request_path =
        paths
            .requests_dir
            .join(format!("{}-{}.json", now_nanos(), request.request_id));
    let temp_path = request_path.with_extension("json.tmp");
    let bytes = serde_json::to_vec(request)?;
    fs::write(&temp_path, bytes)?;
    fs::rename(temp_path, request_path)?;
    Ok(())
}

fn drain_pending_requests<F>(paths: &QueuePaths, executor: &mut F) -> Result<usize, QueueError>
where
    F: FnMut(&QueuedWriteRequest) -> QueuedWriteResponse,
{
    let worker_lock = FileLock::try_acquire(&paths.worker_lock_path)?;
    let Some(_worker_guard) = worker_lock else {
        return Ok(0);
    };

    let mut processed = 0usize;
    loop {
        let request_files = list_request_files(&paths.requests_dir)?;
        if request_files.is_empty() {
            break;
        }
        for request_file in request_files {
            let request = match read_request_file(&request_file) {
                Ok(request) => request,
                Err(err) => {
                    let _ = fs::remove_file(&request_file);
                    return Err(err);
                }
            };
            let response = executor(&request);
            let response_path = PathBuf::from(&request.response_path);
            write_response_file(&response_path, &response)?;
            let _ = fs::remove_file(&request_file);
            processed += 1;
        }
    }

    Ok(processed)
}

fn list_request_files(requests_dir: &Path) -> Result<Vec<PathBuf>, QueueError> {
    let mut files = Vec::new();
    if !requests_dir.exists() {
        return Ok(files);
    }
    for entry in fs::read_dir(requests_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn read_request_file(path: &Path) -> Result<QueuedWriteRequest, QueueError> {
    let bytes = fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn write_response_file(path: &Path, response: &QueuedWriteResponse) -> Result<(), QueueError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec(response)?;
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)?;
    Ok(())
}

fn read_response_file(path: &Path) -> Result<Option<QueuedWriteResponse>, QueueError> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(path)?;
    let response = serde_json::from_slice(&bytes)?;
    Ok(Some(response))
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after UNIX_EPOCH")
        .as_nanos()
}

#[cfg(test)]
mod tests;
