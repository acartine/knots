//! Tests for `lease_expiry_ts` replication on `idx.knot_head` (knot e045).
//!
//! `lease_expiry_ts` used to be a local-only DB column: nothing under
//! `src/sync/` wrote it, so a pulled lease always cached expiry 0 and
//! `effective_lease_state` read it as expired regardless of the owner's
//! real remaining time. These tests pin the fix: the field now round-trips
//! through `idx.knot_head`, and an event that omits it (pre-fix binary, or
//! a knot with no lease) preserves whatever is already cached instead of
//! resetting it.

use std::path::{Path, PathBuf};
use std::process::Command;

use uuid::Uuid;

use crate::db;
use crate::lease_expiry::effective_lease_state;
use crate::sync::GitAdapter;

use super::IncrementalApplier;

fn unique_workspace() -> knots_test_support::TestWorkspace {
    knots_test_support::workspace("knots-sync-lease-expiry")
}

fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn setup_repo() -> knots_test_support::TestWorkspace {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    run_git(&root, &["init"]);
    run_git(&root, &["config", "user.email", "knots@example.com"]);
    run_git(&root, &["config", "user.name", "Knots Test"]);
    std::fs::write(root.join("README.md"), "# lease-expiry\n").expect("readme should be writable");
    run_git(&root, &["add", "README.md"]);
    run_git(&root, &["commit", "-m", "init"]);
    root_ws
}

fn open_conn(root: &Path) -> rusqlite::Connection {
    let db_path = root.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(db_path.parent().expect("db parent should exist"))
        .expect("db parent should be creatable");
    db::open_connection(db_path.to_str().expect("utf8 db path")).expect("db should open")
}

fn write_head_event(root: &Path, filename: &str, body: &str) -> PathBuf {
    let idx_dir = root.join(".knots/index/2026/02/10");
    std::fs::create_dir_all(&idx_dir).expect("index dir creatable");
    let path = idx_dir.join(filename);
    std::fs::write(&path, body).expect("event should be writable");
    Path::new(".knots/index/2026/02/10").join(filename)
}

/// Fresh-enough timestamp to keep a non-terminal event in the hot tier
/// under a long hot window, so the test can inspect `knot_hot` directly.
fn recent_ts() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("timestamp should format")
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_secs() as i64
}

fn head_event_body(ts: &str, knot_id: &str, state: &str, lease_expiry_field: &str) -> String {
    format!(
        concat!(
            "{{\n",
            "  \"event_id\": \"{event_id}\",\n",
            "  \"occurred_at\": \"{ts}\",\n",
            "  \"type\": \"idx.knot_head\",\n",
            "  \"data\": {{\n",
            "    \"knot_id\": \"{knot_id}\",\n",
            "    \"title\": \"Lease\",\n",
            "    \"state\": \"{state}\",\n",
            "    \"terminal\": false,\n",
            "    \"workflow_id\": \"work_sdlc\",\n",
            "    \"profile_id\": \"autopilot\",\n",
            "    \"updated_at\": \"{ts}\"\n",
            "    {lease_expiry_field}\n",
            "  }}\n",
            "}}\n"
        ),
        event_id = Uuid::now_v7(),
        ts = ts,
        knot_id = knot_id,
        state = state,
        lease_expiry_field = lease_expiry_field,
    )
}

/// A lease pushed from machine A with a future expiry is readable on
/// machine B with that same expiry after pull, and reads as non-expired.
#[test]
fn apply_index_event_replicates_future_lease_expiry_and_reads_non_expired() {
    let root_ws = setup_repo();
    let root = root_ws.path().to_path_buf();
    let conn = open_conn(&root);
    db::set_meta(&conn, "hot_window_days", "365").expect("hot window should be configurable");
    let mut applier = IncrementalApplier::new_with_builtins(&conn, root.clone(), GitAdapter::new());

    let ts = recent_ts();
    let future_expiry = now_unix() + 3_600;
    let body = head_event_body(
        &ts,
        "K-lease-remote",
        "lease_active",
        &format!(",\n    \"lease_expiry_ts\": {future_expiry}"),
    );
    let rel = write_head_event(&root, "remote-lease-idx.knot_head.json", &body);

    applier
        .apply_index_event(&rel)
        .expect("head event with a future expiry should apply");

    let record = db::get_knot_hot(&conn, "K-lease-remote")
        .expect("hot lookup should succeed")
        .expect("knot should be cached");
    assert_eq!(record.lease_expiry_ts, future_expiry);
    assert_eq!(
        effective_lease_state(&record.state, record.lease_expiry_ts),
        "lease_active",
        "a lease pulled with a future expiry must not read as expired"
    );
}

/// An index event with no `lease_expiry_ts` field (an older binary, or a
/// knot that was never a lease) yields "unknown" expiry for a knot never
/// seen before: it lands at the schema default (0), which is exactly
/// today's pre-fix behavior, not a new failure mode.
#[test]
fn apply_index_event_missing_lease_expiry_ts_defaults_new_knot_to_unknown() {
    let root_ws = setup_repo();
    let root = root_ws.path().to_path_buf();
    let conn = open_conn(&root);
    db::set_meta(&conn, "hot_window_days", "365").expect("hot window should be configurable");
    let mut applier = IncrementalApplier::new_with_builtins(&conn, root.clone(), GitAdapter::new());

    let ts = recent_ts();
    let body = head_event_body(&ts, "K-lease-legacy", "lease_active", "");
    let rel = write_head_event(&root, "legacy-lease-idx.knot_head.json", &body);

    applier
        .apply_index_event(&rel)
        .expect("head event missing lease_expiry_ts should still apply");

    let record = db::get_knot_hot(&conn, "K-lease-legacy")
        .expect("hot lookup should succeed")
        .expect("knot should be cached");
    assert_eq!(record.lease_expiry_ts, 0);
    assert_eq!(
        effective_lease_state(&record.state, record.lease_expiry_ts),
        "lease_terminated",
        "absent expiry must preserve today's 0-as-expired legacy behavior"
    );
}

/// A later event that omits `lease_expiry_ts` must not clobber a value a
/// prior event already established for this knot -- "absent" means
/// "unknown to this event", not "reset to expired".
#[test]
fn apply_index_event_missing_lease_expiry_ts_preserves_previously_known_value() {
    let root_ws = setup_repo();
    let root = root_ws.path().to_path_buf();
    let conn = open_conn(&root);
    db::set_meta(&conn, "hot_window_days", "365").expect("hot window should be configurable");
    let mut applier = IncrementalApplier::new_with_builtins(&conn, root.clone(), GitAdapter::new());

    let ts1 = recent_ts();
    let future_expiry = now_unix() + 3_600;
    let first_body = head_event_body(
        &ts1,
        "K-lease-preserve",
        "lease_active",
        &format!(",\n    \"lease_expiry_ts\": {future_expiry}"),
    );
    let first_rel = write_head_event(&root, "preserve-1-idx.knot_head.json", &first_body);
    applier
        .apply_index_event(&first_rel)
        .expect("first head event should apply");

    // A later event on the same knot (e.g. a note-triggered head refresh
    // written by an older binary) that carries no lease_expiry_ts at all.
    let ts2 = recent_ts();
    let second_body = head_event_body(&ts2, "K-lease-preserve", "lease_active", "");
    let second_rel = write_head_event(&root, "preserve-2-idx.knot_head.json", &second_body);
    applier
        .apply_index_event(&second_rel)
        .expect("second head event should apply");

    let record = db::get_knot_hot(&conn, "K-lease-preserve")
        .expect("hot lookup should succeed")
        .expect("knot should be cached");
    assert_eq!(
        record.lease_expiry_ts, future_expiry,
        "an event without the field must not reset a previously known expiry"
    );
}
