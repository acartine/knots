use super::{
    canonical_binary_path, format_titled_fields, format_upgrade_summary, paint, parent_dir,
    remove_file_if_present, resolve_binary_path, run_uninstall, run_update, update_install_dir,
    upgrade_hint_needed, SelfUninstallOptions, SelfUpdateOptions,
};
use std::path::Path;

#[cfg(unix)]
fn symlink_file(src: &Path, dst: &Path) {
    std::os::unix::fs::symlink(src, dst).expect("symlink should be created");
}

#[cfg(windows)]
fn symlink_file(src: &Path, dst: &Path) {
    std::os::windows::fs::symlink_file(src, dst).expect("symlink should be created");
}

fn unique_temp_dir() -> knots_test_support::TestWorkspace {
    knots_test_support::workspace("knots-self-manage")
}

#[test]
fn uninstall_removes_binary_and_previous_when_requested() {
    let dir_ws = unique_temp_dir();
    let dir = dir_ws.path().to_path_buf();
    let binary = dir.join("knots");
    let alias = dir.join("kno");
    let previous = dir.join("kno.previous");
    let legacy_previous = dir.join("knots.previous");
    std::fs::write(&binary, b"bin").expect("binary fixture should be written");
    symlink_file(&binary, &alias);
    std::fs::write(&previous, b"bin").expect("previous fixture should be written");
    std::fs::write(&legacy_previous, b"bin").expect("legacy previous fixture should be written");

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
}

#[test]
fn uninstall_keeps_previous_without_flag() {
    let dir_ws = unique_temp_dir();
    let dir = dir_ws.path().to_path_buf();
    let binary = dir.join("knots");
    let alias = dir.join("kno");
    let previous = dir.join("kno.previous");
    let legacy_previous = dir.join("knots.previous");
    std::fs::write(&binary, b"bin").expect("binary fixture should be written");
    symlink_file(&binary, &alias);
    std::fs::write(&previous, b"bin").expect("previous fixture should be written");
    std::fs::write(&legacy_previous, b"bin").expect("legacy previous fixture should be written");

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
}

#[test]
fn update_and_path_helpers_cover_error_paths() {
    let dir_ws = unique_temp_dir();
    let dir = dir_ws.path().to_path_buf();
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

    let missing_installer = dir.join("missing-installer.sh");
    let missing_result = run_update(&SelfUpdateOptions {
        version: None,
        repo: None,
        install_dir: Some(dir.clone()),
        script_url: format!("file://{}", missing_installer.display()),
    });
    assert!(missing_result.is_err());

    let current = resolve_binary_path(None).expect("current executable path should resolve");
    // Under coverage tools the test binary may be a temporary path that no
    // longer exists after instrumentation, so only assert the path resolved.
    assert!(!current.as_os_str().is_empty());

    let missing = dir.join("missing-knots-binary");
    let uninstall = run_uninstall(&SelfUninstallOptions {
        bin_path: Some(missing),
        remove_previous: false,
    });
    assert!(uninstall.is_err());
}

#[test]
fn update_install_dir_uses_explicit_or_running_binary_parent() {
    let dir_ws = unique_temp_dir();
    let dir = dir_ws.path().to_path_buf();
    assert_eq!(
        update_install_dir(Some(dir.clone())).expect("explicit install dir should resolve"),
        dir
    );

    let implicit = update_install_dir(None).expect("implicit install dir should resolve");
    assert!(!implicit.as_os_str().is_empty());
}

#[test]
fn parent_dir_falls_back_to_dot_for_bare_paths() {
    assert_eq!(parent_dir(Path::new("knots")), Path::new("."));
    assert_eq!(
        parent_dir(Path::new("/example/knots")),
        Path::new("/example")
    );
}

#[test]
fn canonicalize_and_remove_file_helpers_cover_directory_and_missing_paths() {
    let dir_ws = unique_temp_dir();
    let dir = dir_ws.path().to_path_buf();
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
        let denied =
            remove_file_if_present(&denied_path).expect_err("permission denied path should fail");
        assert_ne!(denied.kind(), std::io::ErrorKind::NotFound);

        let mut reset = std::fs::metadata(&locked)
            .expect("locked dir metadata should be readable")
            .permissions();
        reset.set_mode(0o755);
        std::fs::set_permissions(&locked, reset).expect("locked dir permissions should reset");
    }
}

#[test]
fn upgrade_summary_right_aligns_labels_and_left_aligns_values() {
    std::env::set_var("NO_COLOR", "1");
    let install_dir = Path::new("/example/kno-test-install");
    let summary = format_upgrade_summary(
        Some("v1.2.3"),
        Some("acartine/knots"),
        Some(install_dir),
        true,
    );
    std::env::remove_var("NO_COLOR");
    let lines = summary.lines().collect::<Vec<_>>();
    assert_eq!(lines[0], "Upgrade");
    assert_eq!(lines[1], "     status:  updated kno binary");
    assert_eq!(lines[2], "    version:  v1.2.3");
    assert_eq!(lines[3], "       repo:  acartine/knots");
    assert_eq!(lines[4], "install_dir:  /example/kno-test-install");
    assert!(lines[5].contains("kno doctor"));
}

#[test]
fn upgrade_summary_omits_hint_when_not_needed() {
    let install_dir = Path::new("/example/kno-test-install");
    let summary = format_upgrade_summary(
        Some("v1.2.3"),
        Some("acartine/knots"),
        Some(install_dir),
        false,
    );
    assert!(!summary.contains("kno doctor"));
}

#[test]
fn upgrade_hint_needed_stays_enabled_outside_git_repo() {
    let dir_ws = unique_temp_dir();
    let dir = dir_ws.path().to_path_buf();
    assert!(upgrade_hint_needed(&dir));
}

#[test]
fn titled_fields_render_plain_text_when_color_is_disabled() {
    std::env::set_var("NO_COLOR", "1");
    let rendered = format_titled_fields("Upgrade", &[("status", "updated kno binary".to_string())]);
    std::env::remove_var("NO_COLOR");
    assert_eq!(rendered.lines().next(), Some("Upgrade"));
    assert!(!rendered.contains("\x1b["));
    assert!(rendered.contains("status:  updated kno binary"));
}

#[test]
fn paint_respects_no_color() {
    std::env::set_var("NO_COLOR", "1");
    let rendered = paint("1;36", "Upgrade");
    std::env::remove_var("NO_COLOR");
    assert_eq!(rendered, "Upgrade");
}
