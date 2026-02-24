use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Default)]
pub struct SelfUpdateOptions {
    pub version: Option<String>,
    pub repo: Option<String>,
    pub install_dir: Option<PathBuf>,
    pub script_url: String,
}

#[derive(Debug, Clone, Default)]
pub struct SelfUninstallOptions {
    pub bin_path: Option<PathBuf>,
    pub remove_previous: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallResult {
    pub binary_path: PathBuf,
    pub removed_previous: bool,
    pub removed_aliases: Vec<PathBuf>,
}

pub fn run_update(options: &SelfUpdateOptions) -> io::Result<()> {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg("curl -fsSL \"$1\" | sh")
        .arg("knots-self-update")
        .arg(&options.script_url);
    apply_update_env(&mut command, options);

    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "update installer failed with status {status}"
        )))
    }
}

pub fn run_uninstall(options: &SelfUninstallOptions) -> io::Result<UninstallResult> {
    let launch_path = resolve_binary_path(options.bin_path.clone())?;
    if std::fs::symlink_metadata(&launch_path).is_err() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("binary not found: {}", launch_path.display()),
        ));
    }
    let binary_path = canonical_binary_path(&launch_path)?;

    remove_file_if_present(&binary_path)?;
    let mut removed_aliases = Vec::new();
    for alias_path in alias_paths(&binary_path) {
        if alias_path == binary_path {
            continue;
        }
        if remove_file_if_present(&alias_path)? {
            removed_aliases.push(alias_path);
        }
    }

    let mut removed_previous = false;
    if options.remove_previous {
        for previous_path in previous_paths(&binary_path) {
            if remove_file_if_present(&previous_path)? {
                removed_previous = true;
            }
        }
    }

    Ok(UninstallResult {
        binary_path,
        removed_previous,
        removed_aliases,
    })
}

fn apply_update_env(command: &mut Command, options: &SelfUpdateOptions) {
    if let Some(version) = options.version.as_deref() {
        command.env("KNOTS_VERSION", version);
    }
    if let Some(repo) = options.repo.as_deref() {
        command.env("KNOTS_GITHUB_REPO", repo);
    }
    if let Some(install_dir) = options.install_dir.as_deref() {
        command.env("KNOTS_INSTALL_DIR", install_dir);
    }
}

fn resolve_binary_path(explicit: Option<PathBuf>) -> io::Result<PathBuf> {
    match explicit {
        Some(path) => Ok(path),
        None => std::env::current_exe(),
    }
}

fn canonical_binary_path(path: &Path) -> io::Result<PathBuf> {
    match std::fs::canonicalize(path) {
        Ok(canonical) => Ok(canonical),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(path.to_path_buf()),
        Err(err) => Err(err),
    }
}

fn remove_file_if_present(path: &Path) -> io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("expected file but found directory: {}", path.display()),
                ));
            }
            std::fs::remove_file(path)?;
            Ok(true)
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

fn alias_paths(binary_path: &Path) -> Vec<PathBuf> {
    let parent = binary_path.parent().unwrap_or_else(|| Path::new("."));
    vec![parent.join("kno"), parent.join("knots")]
}

fn previous_paths(binary_path: &Path) -> Vec<PathBuf> {
    let parent = binary_path.parent().unwrap_or_else(|| Path::new("."));
    vec![parent.join("kno.previous"), parent.join("knots.previous")]
}

#[cfg(test)]
mod tests {
    use super::{run_uninstall, SelfUninstallOptions};
    use std::path::Path;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[cfg(unix)]
    fn symlink_file(src: &Path, dst: &Path) {
        std::os::unix::fs::symlink(src, dst).expect("symlink should be created");
    }

    #[cfg(windows)]
    fn symlink_file(src: &Path, dst: &Path) {
        std::os::windows::fs::symlink_file(src, dst).expect("symlink should be created");
    }

    fn unique_temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after UNIX_EPOCH")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("knots-self-manage-{nanos}"));
        std::fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn uninstall_removes_binary_and_previous_when_requested() {
        let dir = unique_temp_dir();
        let binary = dir.join("knots");
        let alias = dir.join("kno");
        let previous = dir.join("kno.previous");
        let legacy_previous = dir.join("knots.previous");
        std::fs::write(&binary, b"bin").expect("binary fixture should be written");
        symlink_file(&binary, &alias);
        std::fs::write(&previous, b"bin").expect("previous fixture should be written");
        std::fs::write(&legacy_previous, b"bin")
            .expect("legacy previous fixture should be written");

        let result = run_uninstall(&SelfUninstallOptions {
            bin_path: Some(alias.clone()),
            remove_previous: true,
        })
        .expect("uninstall should succeed");

        assert_eq!(
            result.binary_path.file_name().and_then(|value| value.to_str()),
            Some("knots")
        );
        assert!(result.removed_previous);
        assert_eq!(result.removed_aliases.len(), 1);
        assert_eq!(
            result.removed_aliases[0]
                .file_name()
                .and_then(|value| value.to_str()),
            Some("kno")
        );
        assert!(!result.binary_path.exists());
        assert!(!alias.exists());
        assert!(!previous.exists());
        assert!(!legacy_previous.exists());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn uninstall_keeps_previous_without_flag() {
        let dir = unique_temp_dir();
        let binary = dir.join("knots");
        let alias = dir.join("kno");
        let previous = dir.join("kno.previous");
        let legacy_previous = dir.join("knots.previous");
        std::fs::write(&binary, b"bin").expect("binary fixture should be written");
        symlink_file(&binary, &alias);
        std::fs::write(&previous, b"bin").expect("previous fixture should be written");
        std::fs::write(&legacy_previous, b"bin")
            .expect("legacy previous fixture should be written");

        let result = run_uninstall(&SelfUninstallOptions {
            bin_path: Some(binary),
            remove_previous: false,
        })
        .expect("uninstall should succeed");

        assert!(!result.binary_path.exists());
        assert!(!result.removed_previous);
        assert!(!alias.exists());
        assert!(previous.exists());
        assert!(legacy_previous.exists());

        let _ = std::fs::remove_dir_all(dir);
    }
}
