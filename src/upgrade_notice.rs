use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::project;
use crate::release_version::{fetch_latest_tag, latest_available_version, RELEASES_LATEST_URL};

const CHECK_INTERVAL_SECS: i64 = 24 * 60 * 60;
const CHECK_TIMEOUT_SECS: u32 = 2;
const STATE_FILE_NAME: &str = "upgrade-check.json";

#[derive(Debug, Deserialize, Serialize)]
struct UpgradeCheckState {
    last_checked_unix_secs: i64,
}

pub(crate) fn maybe_print_upgrade_notice() {
    if std::env::var_os("KNOTS_SKIP_DOCTOR_UPGRADE").is_some() {
        return;
    }
    let Some(state_path) = state_path() else {
        return;
    };
    if let Some(banner) = maybe_upgrade_banner_at(
        &state_path,
        env!("CARGO_PKG_VERSION"),
        current_unix_secs(),
        |timeout_secs| fetch_latest_tag(RELEASES_LATEST_URL, timeout_secs),
    ) {
        eprintln!("{banner}");
    }
}

fn maybe_upgrade_banner_at<F>(
    state_path: &Path,
    current_version: &str,
    now_unix_secs: i64,
    fetch_latest: F,
) -> Option<String>
where
    F: FnOnce(u32) -> Option<String>,
{
    if !should_check(read_last_checked(state_path), now_unix_secs) {
        return None;
    }
    let latest_tag = fetch_latest(CHECK_TIMEOUT_SECS);
    let _ = write_last_checked(state_path, now_unix_secs);
    let latest = latest_available_version(current_version, latest_tag)?;
    Some(format_banner(current_version, &latest))
}

fn state_path() -> Option<PathBuf> {
    project::data_dir(None)
        .ok()
        .map(|data_dir| data_dir.join(STATE_FILE_NAME))
}

fn current_unix_secs() -> i64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => duration.as_secs() as i64,
        Err(_) => 0,
    }
}

fn read_last_checked(state_path: &Path) -> Option<i64> {
    let raw = fs::read_to_string(state_path).ok()?;
    let state: UpgradeCheckState = serde_json::from_str(&raw).ok()?;
    Some(state.last_checked_unix_secs)
}

fn should_check(last_checked_unix_secs: Option<i64>, now_unix_secs: i64) -> bool {
    match last_checked_unix_secs {
        None => true,
        Some(last_checked) if now_unix_secs <= last_checked => false,
        Some(last_checked) => now_unix_secs - last_checked >= CHECK_INTERVAL_SECS,
    }
}

fn write_last_checked(state_path: &Path, now_unix_secs: i64) -> std::io::Result<()> {
    if let Some(parent) = state_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let state = UpgradeCheckState {
        last_checked_unix_secs: now_unix_secs,
    };
    let bytes = serde_json::to_vec(&state)?;
    fs::write(state_path, bytes)
}

fn format_banner(current_version: &str, latest_version: &str) -> String {
    format!("Upgrade available: v{current_version} -> v{latest_version} (run `kno upgrade`)")
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::path::Path;
    use std::path::PathBuf;

    use uuid::Uuid;

    use super::{
        current_unix_secs, format_banner, maybe_upgrade_banner_at, read_last_checked, should_check,
        state_path as global_state_path, write_last_checked, UpgradeCheckState, CHECK_TIMEOUT_SECS,
    };

    fn state_path(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("knots-upgrade-notice-{}-{name}", Uuid::now_v7()))
            .join("upgrade-check.json")
    }

    fn read_state_file(path: &PathBuf) -> UpgradeCheckState {
        let raw = std::fs::read_to_string(path).expect("state file should exist");
        serde_json::from_str(&raw).expect("state file should be valid")
    }

    #[test]
    fn first_run_checks_and_records_timestamp() {
        let path = state_path("first-run");
        let calls = Cell::new(0);
        let banner = maybe_upgrade_banner_at(&path, "0.11.0", 10, |timeout_secs| {
            calls.set(calls.get() + 1);
            assert_eq!(timeout_secs, CHECK_TIMEOUT_SECS);
            Some("v0.12.0".to_string())
        });
        assert_eq!(
            banner,
            Some("Upgrade available: v0.11.0 -> v0.12.0 (run `kno upgrade`)".to_string())
        );
        assert_eq!(calls.get(), 1);
        assert_eq!(read_state_file(&path).last_checked_unix_secs, 10);
        let _ = std::fs::remove_dir_all(path.parent().expect("temp dir should exist"));
    }

    #[test]
    fn stale_check_triggers_recheck() {
        let path = state_path("stale");
        std::fs::create_dir_all(path.parent().expect("temp dir should exist"))
            .expect("temp dir should be creatable");
        std::fs::write(&path, r#"{"last_checked_unix_secs":1}"#)
            .expect("state file should be writable");

        let calls = Cell::new(0);
        let banner = maybe_upgrade_banner_at(&path, "0.11.0", 1 + 24 * 60 * 60, |_| {
            calls.set(calls.get() + 1);
            Some("v0.12.0".to_string())
        });
        assert!(banner.is_some());
        assert_eq!(calls.get(), 1);
        assert_eq!(
            read_state_file(&path).last_checked_unix_secs,
            1 + 24 * 60 * 60
        );
        let _ = std::fs::remove_dir_all(path.parent().expect("temp dir should exist"));
    }

    #[test]
    fn fresh_check_is_skipped() {
        let path = state_path("fresh");
        std::fs::create_dir_all(path.parent().expect("temp dir should exist"))
            .expect("temp dir should be creatable");
        std::fs::write(&path, r#"{"last_checked_unix_secs":90}"#)
            .expect("state file should be writable");

        let calls = Cell::new(0);
        let banner = maybe_upgrade_banner_at(&path, "0.11.0", 100, |_| {
            calls.set(calls.get() + 1);
            Some("v0.12.0".to_string())
        });
        assert_eq!(banner, None);
        assert_eq!(calls.get(), 0);
        assert_eq!(read_state_file(&path).last_checked_unix_secs, 90);
        let _ = std::fs::remove_dir_all(path.parent().expect("temp dir should exist"));
    }

    #[test]
    fn newer_version_available_returns_banner() {
        let path = state_path("newer");
        let banner = maybe_upgrade_banner_at(&path, "0.11.0", 10, |_| Some("v0.12.0".to_string()));
        assert_eq!(banner, Some(format_banner("0.11.0", "0.12.0")));
        let _ = std::fs::remove_dir_all(path.parent().expect("temp dir should exist"));
    }

    #[test]
    fn up_to_date_version_suppresses_banner_and_records_check() {
        let path = state_path("current");
        let banner = maybe_upgrade_banner_at(&path, "0.11.0", 20, |_| Some("v0.11.0".to_string()));
        assert_eq!(banner, None);
        assert_eq!(read_state_file(&path).last_checked_unix_secs, 20);
        let _ = std::fs::remove_dir_all(path.parent().expect("temp dir should exist"));
    }

    #[test]
    fn network_failure_suppresses_banner_and_records_check() {
        let path = state_path("network-failure");
        let banner = maybe_upgrade_banner_at(&path, "0.11.0", 30, |_| None);
        assert_eq!(banner, None);
        assert_eq!(read_state_file(&path).last_checked_unix_secs, 30);
        let _ = std::fs::remove_dir_all(path.parent().expect("temp dir should exist"));
    }

    #[test]
    fn helpers_cover_check_window_and_state_reads() {
        assert!(should_check(None, 10));
        assert!(!should_check(Some(10), 10));
        assert!(!should_check(Some(20), 10));
        assert!(!should_check(Some(10), 10 + 60));
        assert!(should_check(Some(10), 10 + 24 * 60 * 60));

        let path = state_path("read-last-checked");
        std::fs::create_dir_all(path.parent().expect("temp dir should exist"))
            .expect("temp dir should be creatable");
        std::fs::write(&path, r#"{"last_checked_unix_secs":42}"#)
            .expect("state file should be writable");
        assert_eq!(read_last_checked(&path), Some(42));
        let _ = std::fs::remove_dir_all(path.parent().expect("temp dir should exist"));
    }

    #[test]
    fn current_unix_secs_is_non_negative() {
        assert!(current_unix_secs() >= 0);
    }

    #[test]
    fn state_path_ends_with_upgrade_check_filename() {
        let path = global_state_path().expect("state path should resolve");
        assert!(path.ends_with("upgrade-check.json"));
    }

    #[test]
    fn read_last_checked_returns_none_for_missing_or_invalid_state() {
        let path = state_path("invalid-state");
        assert_eq!(read_last_checked(&path), None);

        std::fs::create_dir_all(path.parent().expect("temp dir should exist"))
            .expect("temp dir should be creatable");
        std::fs::write(&path, "not-json").expect("state file should be writable");
        assert_eq!(read_last_checked(&path), None);
        let _ = std::fs::remove_dir_all(path.parent().expect("temp dir should exist"));
    }

    #[test]
    fn write_last_checked_supports_paths_without_parent() {
        let path = PathBuf::from(format!("upgrade-check-{}.json", Uuid::now_v7()));
        write_last_checked(Path::new(&path), 55).expect("state file should be writable");
        assert_eq!(read_last_checked(Path::new(&path)), Some(55));
        let _ = std::fs::remove_file(path);
    }
}
