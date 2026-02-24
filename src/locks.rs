use std::fs::{File, OpenOptions};
use std::io::ErrorKind;
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
        Ok(file) => Ok(Some(FileLock {
            path: path.to_path_buf(),
            _file: file,
        })),
        Err(err) if err.kind() == ErrorKind::AlreadyExists => Ok(None),
        Err(err) => Err(LockError::Io(err)),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;
    use uuid::Uuid;

    use super::FileLock;

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
}
