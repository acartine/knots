//! Tests for legacy-tolerance fallbacks in `apply_index_event`.
//!
//! Events committed to origin before 2026-04-09 may omit `profile_id`
//! entirely or carry pre-registry workflow names like `"default"`. The
//! apply path has to translate these to modern equivalents at read time so
//! a bootstrap pull of an old repo doesn't hard-fail.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::db;
use crate::domain::scope::ScopeData;
use crate::sync::GitAdapter;

use super::IncrementalApplier;

fn unique_workspace() -> knots_test_support::TestWorkspace {
    knots_test_support::workspace("knots-sync-legacy")
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
    std::fs::write(root.join("README.md"), "# legacy\n").expect("readme should be writable");
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

fn write_legacy_head_event(root: &Path, filename: &str, body: &str) -> PathBuf {
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

#[test]
fn apply_index_event_defaults_missing_profile_id_to_autopilot() {
    // Pre-2026-04-09 event shape: no profile_id field at all. Must apply
    // and land in the cache with profile_id="autopilot".
    let root_ws = setup_repo();
    let root = root_ws.path().to_path_buf();
    let conn = open_conn(&root);
    db::set_meta(&conn, "hot_window_days", "365").expect("hot window should be configurable");
    let mut applier = IncrementalApplier::new_with_builtins(&conn, root.clone(), GitAdapter::new());

    let ts = recent_ts();
    let body = format!(
        concat!(
            "{{\n",
            "  \"event_id\": \"019c942e-2369-7883-bb13-27197273b8f5\",\n",
            "  \"occurred_at\": \"{ts}\",\n",
            "  \"type\": \"idx.knot_head\",\n",
            "  \"data\": {{\n",
            "    \"knot_id\": \"K-legacy-no-profile\",\n",
            "    \"title\": \"Legacy knot\",\n",
            "    \"state\": \"implementation\",\n",
            "    \"terminal\": false,\n",
            "    \"workflow_id\": \"work_sdlc\",\n",
            "    \"updated_at\": \"{ts}\"\n",
            "  }}\n",
            "}}\n"
        ),
        ts = ts,
    );
    let rel = write_legacy_head_event(&root, "legacy-no-profile-idx.knot_head.json", &body);

    applier
        .apply_index_event(&rel)
        .expect("legacy event without profile_id should apply, not fail");

    let record = db::get_knot_hot(&conn, "K-legacy-no-profile")
        .expect("hot lookup should succeed")
        .expect("knot should be cached");
    assert_eq!(record.profile_id, "autopilot");
    assert_eq!(record.scope_data, ScopeData::default());
}

#[test]
fn apply_index_event_reads_scope_payload_into_hot_projection() {
    let root_ws = setup_repo();
    let root = root_ws.path().to_path_buf();
    let conn = open_conn(&root);
    db::set_meta(&conn, "hot_window_days", "365").expect("hot window should be configurable");
    let mut applier = IncrementalApplier::new_with_builtins(&conn, root.clone(), GitAdapter::new());

    let ts = recent_ts();
    let body = format!(
        concat!(
            "{{\n",
            "  \"event_id\": \"019c942e-0000-7883-bb13-27197273b8f5\",\n",
            "  \"occurred_at\": \"{ts}\",\n",
            "  \"type\": \"idx.knot_head\",\n",
            "  \"data\": {{\n",
            "    \"knot_id\": \"K-index-scope\",\n",
            "    \"title\": \"Scoped index knot\",\n",
            "    \"state\": \"implementation\",\n",
            "    \"terminal\": false,\n",
            "    \"workflow_id\": \"work_sdlc\",\n",
            "    \"profile_id\": \"autopilot\",\n",
            "    \"updated_at\": \"{ts}\",\n",
            "    \"scope\": {{\n",
            "      \"volume\": 21,\n",
            "      \"scale\": \"fib_v1\",\n",
            "      \"reliability\": 88,\n",
            "      \"reliability_band\": \"high\"\n",
            "    }}\n",
            "  }}\n",
            "}}\n"
        ),
        ts = ts,
    );
    let rel = write_legacy_head_event(&root, "index-scope-idx.knot_head.json", &body);

    applier
        .apply_index_event(&rel)
        .expect("idx head with scope should apply");

    let record = db::get_knot_hot(&conn, "K-index-scope")
        .expect("hot lookup should succeed")
        .expect("knot should be cached");
    assert_eq!(record.scope_data.volume, Some(21));
    assert_eq!(record.scope_data.scale.as_deref(), Some("fib_v1"));
    assert_eq!(record.scope_data.reliability, Some(88));
    assert_eq!(record.scope_data.reliability_band.as_deref(), Some("high"));
}

#[test]
fn apply_index_event_converts_legacy_default_workflow_id_to_work_sdlc() {
    // The pre-workflow-registry name "default" must be translated the
    // same way "compatibility" and "knots_sdlc" already are.
    let root_ws = setup_repo();
    let root = root_ws.path().to_path_buf();
    let conn = open_conn(&root);
    db::set_meta(&conn, "hot_window_days", "365").expect("hot window should be configurable");
    let mut applier = IncrementalApplier::new_with_builtins(&conn, root.clone(), GitAdapter::new());

    let ts = recent_ts();
    let body = format!(
        concat!(
            "{{\n",
            "  \"event_id\": \"019c942e-aaaa-7883-bb13-27197273b8f5\",\n",
            "  \"occurred_at\": \"{ts}\",\n",
            "  \"type\": \"idx.knot_head\",\n",
            "  \"data\": {{\n",
            "    \"knot_id\": \"K-default-wf\",\n",
            "    \"title\": \"Legacy knot\",\n",
            "    \"state\": \"implementation\",\n",
            "    \"terminal\": false,\n",
            "    \"workflow_id\": \"default\",\n",
            "    \"profile_id\": \"autopilot\",\n",
            "    \"updated_at\": \"{ts}\"\n",
            "  }}\n",
            "}}\n"
        ),
        ts = ts,
    );
    let rel = write_legacy_head_event(&root, "legacy-default-wf-idx.knot_head.json", &body);

    applier
        .apply_index_event(&rel)
        .expect("workflow_id=default should convert, not fail");

    let record = db::get_knot_hot(&conn, "K-default-wf")
        .expect("hot lookup should succeed")
        .expect("knot should be cached");
    assert_eq!(record.workflow_id, "work_sdlc");
}

#[test]
fn apply_index_event_accepts_foolery_style_event_with_both_legacy_markers() {
    // Reproduces the foolery-ajv event exactly (both legacy markers,
    // terminal=true). Must apply cleanly — under the old strict code
    // path this hard-failed bootstrap.
    let root_ws = setup_repo();
    let root = root_ws.path().to_path_buf();
    let conn = open_conn(&root);
    db::set_meta(&conn, "hot_window_days", "365").expect("hot window should be configurable");
    let mut applier = IncrementalApplier::new_with_builtins(&conn, root.clone(), GitAdapter::new());

    let body = concat!(
        "{\n",
        "  \"event_id\": \"019c942e-2369-7883-bb13-27197273b8f5\",\n",
        "  \"occurred_at\": \"2026-02-10T09:58:42Z\",\n",
        "  \"type\": \"idx.knot_head\",\n",
        "  \"data\": {\n",
        "    \"knot_id\": \"foolery-ajv\",\n",
        "    \"state\": \"shipped\",\n",
        "    \"terminal\": true,\n",
        "    \"title\": \"Fix BeadTypeBadge crash on unknown type\",\n",
        "    \"updated_at\": \"2026-02-10T09:58:42Z\",\n",
        "    \"workflow_id\": \"default\"\n",
        "  }\n",
        "}\n"
    );
    let rel = write_legacy_head_event(&root, "foolery-ajv-idx.knot_head.json", body);

    applier
        .apply_index_event(&rel)
        .expect("doubly-legacy event should apply, not fail bootstrap");

    // Terminal events land in cold_catalog, not knot_hot.
    let cold = db::get_cold_catalog(&conn, "foolery-ajv")
        .expect("cold lookup should succeed")
        .expect("terminal knot should land in cold catalog");
    assert_eq!(cold.state, "shipped");
}
