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

pub fn maybe_run_self_command(
    command: &crate::cli::Commands,
) -> Result<Option<String>, crate::app::AppError> {
    use crate::cli::Commands;

    match command {
        Commands::Upgrade(update_args) => {
            run_update(&SelfUpdateOptions {
                version: update_args.version.clone(),
                repo: update_args.repo.clone(),
                install_dir: update_args.install_dir.clone(),
                script_url: update_args.script_url.clone(),
            })?;
            Ok(Some("updated kno binary".to_string()))
        }
        Commands::Uninstall(uninstall_args) => {
            let result = run_uninstall(&SelfUninstallOptions {
                bin_path: uninstall_args.bin_path.clone(),
                remove_previous: uninstall_args.remove_previous,
            })?;
            let mut lines = vec![format!("removed {}", result.binary_path.display())];
            if result.removed_previous {
                lines.push("removed previous backups (kno.previous/knots.previous)".to_string());
            }
            Ok(Some(lines.join("\n")))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        canonical_binary_path, remove_file_if_present, resolve_binary_path, run_uninstall,
        run_update, SelfUninstallOptions, SelfUpdateOptions,
    };
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
            result
                .binary_path
                .file_name()
                .and_then(|value| value.to_str()),
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

    #[test]
    fn update_and_path_helpers_cover_error_paths() {
        let dir = unique_temp_dir();
        let installer = dir.join("installer.sh");
        std::fs::write(&installer, "#!/bin/sh\nexit 1\n")
            .expect("installer script fixture should be written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&installer)
                .expect("installer metadata should be readable")
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&installer, perms)
                .expect("installer permissions should be writable");
        }

        let result = run_update(&SelfUpdateOptions {
            version: Some("v0.0.0-test".to_string()),
            repo: Some("acartine/knots".to_string()),
            install_dir: Some(dir.clone()),
            script_url: format!("file://{}", installer.display()),
        });
        assert!(result.is_err());

        let current = resolve_binary_path(None).expect("current executable path should resolve");
        assert!(current.exists());

        let missing = dir.join("missing-knots-binary");
        let uninstall = run_uninstall(&SelfUninstallOptions {
            bin_path: Some(missing),
            remove_previous: false,
        });
        assert!(uninstall.is_err());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn canonicalize_and_remove_file_helpers_cover_directory_and_missing_paths() {
        let dir = unique_temp_dir();
        let fixture_dir = dir.join("directory-fixture");
        std::fs::create_dir_all(&fixture_dir).expect("fixture directory should be creatable");

        let removed_missing = remove_file_if_present(&dir.join("missing-file"))
            .expect("missing files should be treated as absent");
        assert!(!removed_missing);

        let err = remove_file_if_present(&fixture_dir).expect_err("directory should be rejected");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);

        #[cfg(unix)]
        {
            use std::os::unix::fs::{symlink, PermissionsExt};
            let loop_path = dir.join("loop");
            symlink(&loop_path, &loop_path).expect("symlink loop fixture should be creatable");
            let loop_err = canonical_binary_path(&loop_path).expect_err("symlink loop should fail");
            assert_ne!(loop_err.kind(), std::io::ErrorKind::NotFound);

            let locked = dir.join("locked");
            std::fs::create_dir_all(&locked).expect("locked dir should be creatable");
            let mut perms = std::fs::metadata(&locked)
                .expect("locked dir metadata should be readable")
                .permissions();
            perms.set_mode(0o000);
            std::fs::set_permissions(&locked, perms).expect("locked dir permissions should update");
            let denied_path = locked.join("missing");
            let denied = remove_file_if_present(&denied_path)
                .expect_err("permission denied path should fail");
            assert_ne!(denied.kind(), std::io::ErrorKind::NotFound);

            let mut reset = std::fs::metadata(&locked)
                .expect("locked dir metadata should be readable")
                .permissions();
            reset.set_mode(0o755);
            std::fs::set_permissions(&locked, reset).expect("locked dir permissions should reset");
        }

        let _ = std::fs::remove_dir_all(dir);
    }
}
