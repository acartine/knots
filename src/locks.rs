use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub enum LockError {
    Busy(PathBuf),
    Io(std::io::Error),
}

impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LockError::Busy(path) => write!(f, "lock busy: {}", path.display()),
            LockError::Io(err) => write!(f, "lock I/O error: {}", err),
        }
    }
}

impl std::error::Error for LockError {}

impl From<std::io::Error> for LockError {
    fn from(value: std::io::Error) -> Self {
        LockError::Io(value)
    }
}

#[derive(Debug)]
pub struct FileLock {
    path: PathBuf,
    _file: File,
}

impl FileLock {
    pub fn acquire(path: &Path, timeout: Duration) -> Result<Self, LockError> {
        let start = Instant::now();
        loop {
            match try_acquire(path)? {
                Some(guard) => return Ok(guard),
                None if start.elapsed() >= timeout => {
                    return Err(LockError::Busy(path.to_path_buf()));
                }
                None => thread::sleep(Duration::from_millis(10)),
            }
        }
    }

    pub fn try_acquire(path: &Path) -> Result<Option<Self>, LockError> {
        try_acquire(path)
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn try_acquire(path: &Path) -> Result<Option<FileLock>, LockError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => {
            let _ = write!(file, "{}", std::process::id());
            Ok(Some(FileLock {
                path: path.to_path_buf(),
                _file: file,
            }))
        }
        Err(err) if err.kind() == ErrorKind::AlreadyExists => {
            if reclaim_stale(path)? {
                return try_acquire(path);
            }
            Ok(None)
        }
        Err(err) => Err(LockError::Io(err)),
    }
}

/// Read the PID from the lock file and check if that process is alive.
/// If the process is gone, remove the stale lock and return `true`.
fn reclaim_stale(path: &Path) -> Result<bool, LockError> {
    let mut contents = String::new();
    match File::open(path).and_then(|mut f| f.read_to_string(&mut contents)) {
        Ok(_) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(true),
        Err(_) => return Ok(false),
    }

    let pid: u32 = match contents.trim().parse() {
        Ok(pid) => pid,
        // No valid PID — could be empty or corrupt. Treat as stale.
        Err(_) => {
            let _ = std::fs::remove_file(path);
            return Ok(true);
        }
    };

    if process_alive(pid) {
        Ok(false)
    } else {
        let _ = std::fs::remove_file(path);
        Ok(true)
    }
}

fn process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(pid) = i32::try_from(pid) else {
            return false;
        };
        if pid <= 0 {
            return false;
        }
        // signal 0 checks if the process exists without sending a signal.
        let ret = unsafe { libc_kill(pid, 0) };
        ret == 0
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true // Assume alive on non-unix; timeout will handle it.
    }
}

#[cfg(unix)]
unsafe fn libc_kill(pid: i32, sig: i32) -> i32 {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    unsafe { kill(pid, sig) }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::time::Duration;
    use uuid::Uuid;

    use super::{process_alive, reclaim_stale, FileLock, LockError};

    fn lock_path() -> PathBuf {
        std::env::temp_dir().join(format!("knots-lock-test-{}.lock", Uuid::now_v7()))
    }

    #[test]
    fn try_lock_is_non_blocking() {
        let path = lock_path();
        let first = FileLock::try_acquire(&path)
            .expect("initial lock should not fail")
            .expect("initial lock should succeed");
        let second = FileLock::try_acquire(&path).expect("second lock call should not fail");
        assert!(second.is_none());
        drop(first);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn acquire_times_out_when_held() {
        let path = lock_path();
        let first = FileLock::try_acquire(&path)
            .expect("initial lock should not fail")
            .expect("initial lock should succeed");
        let err = FileLock::acquire(&path, Duration::from_millis(20))
            .expect_err("lock should time out when already held");
        assert!(err.to_string().contains("lock busy"));
        drop(first);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn lock_file_contains_pid() {
        let path = lock_path();
        let _guard = FileLock::try_acquire(&path)
            .expect("lock should not fail")
            .expect("lock should succeed");
        let contents = std::fs::read_to_string(&path).expect("lock file should be readable");
        let pid: u32 = contents
            .trim()
            .parse()
            .expect("lock file should contain a PID");
        assert_eq!(pid, std::process::id());
    }

    #[test]
    fn stale_lock_is_reclaimed() {
        let path = lock_path();
        // Write a PID that doesn't exist (PID 1 is init, use a very
        // high number that almost certainly isn't running).
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent dir should be creatable");
        }
        std::fs::write(&path, "4294967295").expect("stale lock fixture should be writable");
        let reclaimed = reclaim_stale(&path).expect("reclaim should not fail");
        assert!(reclaimed);
        assert!(!path.exists());
    }

    #[test]
    fn corrupt_lock_is_reclaimed() {
        let path = lock_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent dir should be creatable");
        }
        std::fs::write(&path, "not-a-pid").expect("corrupt lock fixture should be writable");
        let reclaimed = reclaim_stale(&path).expect("reclaim should not fail");
        assert!(reclaimed);
        assert!(!path.exists());
    }

    #[test]
    fn live_process_is_not_reclaimed() {
        assert!(process_alive(std::process::id()));
    }

    #[test]
    fn dead_process_is_detected() {
        // PID 4294967295 overflows i32 and should be treated as dead.
        assert!(!process_alive(4294967295));
    }

    #[test]
    fn zero_pid_is_not_alive() {
        assert!(!process_alive(0));
    }

    #[test]
    fn stale_lock_is_reclaimed_via_try_acquire() {
        let path = lock_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent dir should be creatable");
        }
        // Plant a stale lock with a dead PID.
        std::fs::write(&path, "99999").expect("stale lock fixture should be writable");
        // try_acquire should reclaim the stale lock and succeed.
        let guard = FileLock::try_acquire(&path)
            .expect("try_acquire should not fail")
            .expect("stale lock should be reclaimed");
        drop(guard);
    }

    #[test]
    fn reclaim_stale_returns_true_for_missing_file() {
        let path = lock_path();
        // File doesn't exist — reclaim should return true.
        let reclaimed = reclaim_stale(&path).expect("reclaim should not fail");
        assert!(reclaimed);
    }

    #[test]
    fn io_error_paths_surface_as_lock_errors() {
        let path = std::env::temp_dir().join(format!("knots-lock-dir-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&path).expect("directory path should be creatable");

        let converted = LockError::from(std::io::Error::other("boom"));
        assert!(converted.to_string().contains("boom"));

        assert!(!reclaim_stale(&path).expect("directory should not be reclaimed as stale"));
        let _ = std::fs::remove_dir_all(path);
    }

    #[cfg(unix)]
    #[test]
    fn try_acquire_reports_open_errors_from_read_only_directories() {
        let parent = std::env::temp_dir().join(format!("knots-lock-readonly-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&parent).expect("parent dir should be creatable");
        let original = std::fs::metadata(&parent)
            .expect("metadata should be readable")
            .permissions();
        let mut readonly = original.clone();
        readonly.set_mode(0o555);
        std::fs::set_permissions(&parent, readonly).expect("permissions should update");

        let path = parent.join("child.lock");
        let err = super::try_acquire(&path).expect_err("read-only directory should fail");
        assert!(err.to_string().contains("lock I/O error"));

        std::fs::set_permissions(&parent, original).expect("permissions should restore");
        let _ = std::fs::remove_dir_all(parent);
    }
}
