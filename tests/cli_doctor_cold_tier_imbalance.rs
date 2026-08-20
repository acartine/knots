use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use rusqlite::Connection;
use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

fn unique_workspace(prefix: &str) -> knots_test_support::TestWorkspace {
    knots_test_support::workspace(prefix)
}

fn run_git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
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

fn setup_repo_with_remote(root: &Path) {
    run_git(root, &["init"]);
    run_git(root, &["config", "user.email", "knots@example.com"]);
    run_git(root, &["config", "user.name", "Knots Test"]);
    std::fs::write(root.join("README.md"), "# knots\n").expect("readme should be writable");
    run_git(root, &["add", "README.md"]);
    run_git(root, &["commit", "-m", "init"]);
    run_git(root, &["branch", "-M", "main"]);

    let remote = root.join("remote.git");
    run_git(
        root,
        &["init", "--bare", remote.to_str().expect("utf8 path")],
    );
    run_git(
        root,
        &[
            "remote",
            "add",
            "origin",
            remote.to_str().expect("utf8 path"),
        ],
    );
    run_git(root, &["push", "-u", "origin", "main"]);
}

fn knots_binary() -> PathBuf {
    let configured = PathBuf::from(env!("CARGO_BIN_EXE_knots"));
    if configured.is_absolute() && configured.exists() {
        return configured;
    }
    if configured.exists() {
        return std::fs::canonicalize(&configured).unwrap_or(configured);
    }
    let manifest_relative = Path::new(env!("CARGO_MANIFEST_DIR")).join(&configured);
    if manifest_relative.exists() {
        return std::fs::canonicalize(&manifest_relative).unwrap_or(manifest_relative);
    }
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(debug_dir) = current_exe.parent().and_then(|deps| deps.parent()) {
            for name in ["knots", "knots.exe"] {
                let candidate = debug_dir.join(name);
                if candidate.exists() {
                    return candidate;
                }
            }
        }
    }
    configured
}

fn run_knots(repo_root: &Path, db_path: &Path, args: &[&str]) -> Output {
    Command::new(knots_binary())
        .arg("--repo-root")
        .arg(repo_root)
        .arg("--db")
        .arg(db_path)
        .env("KNOTS_SKIP_DOCTOR_UPGRADE", "1")
        .args(args)
        .output()
        .expect("knots command should run")
}

fn fmt_rfc3339(ts: OffsetDateTime) -> String {
    ts.format(&Rfc3339).expect("timestamp should format")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "expected success but failed.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn parse_created_id(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .nth(1)
        .expect("created output should include knot id")
        .to_string()
}

fn create_knot(root: &Path, db: &Path, title: &str) -> String {
    let alias = parse_created_id(&run_knots(
        root,
        db,
        &[
            "new",
            title,
            "--profile",
            "default",
            "--state",
            "implementation",
        ],
    ));
    resolve_full_id(root, db, &alias)
}

fn resolve_full_id(root: &Path, db: &Path, alias: &str) -> String {
    let show = run_knots(root, db, &["show", alias, "--json"]);
    assert_success(&show);
    let json: Value = serde_json::from_slice(&show.stdout).expect("show json should parse");
    json["id"]
        .as_str()
        .expect("show json should carry id")
        .to_string()
}

fn bootstrap_workflows(repo_root: &Path, db_path: &Path) {
    for (knot_type, workflow_id) in [
        ("work", "work_sdlc"),
        ("gate", "gate_sdlc"),
        ("lease", "lease_sdlc"),
        ("explore", "explore_sdlc"),
        ("execution_plan", "execution_plan_sdlc"),
    ] {
        let output = run_knots(
            repo_root,
            db_path,
            &["workflow", "use", workflow_id, "--type", knot_type],
        );
        assert_success(&output);
    }
}

fn demote_to_cold_with_state(db: &Path, id: &str, title: &str, state: &str, updated_at: &str) {
    let conn = Connection::open(db).expect("db should open");
    conn.execute("DELETE FROM knot_hot WHERE id = ?1", rusqlite::params![id])
        .expect("delete hot should succeed");
    conn.execute(
        "INSERT OR REPLACE INTO knot_warm (id, title) VALUES (?1, ?2)",
        rusqlite::params![id, title],
    )
    .expect("insert warm should succeed");
    conn.execute(
        "INSERT OR REPLACE INTO cold_catalog (id, title, state, updated_at) \
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![id, title, state, updated_at],
    )
    .expect("insert cold should succeed");
}

fn demote_to_cold_terminal(db: &Path, id: &str, title: &str, updated_at: &str) {
    demote_to_cold_with_state(db, id, title, "shipped", updated_at);
}

fn insert_stale_terminal_hot(db: &Path, id: &str, title: &str, updated_at: &str) {
    let conn = Connection::open(db).expect("db should open");
    conn.execute(
        "UPDATE knot_hot SET state = 'shipped', updated_at = ?2 WHERE id = ?1",
        rusqlite::params![id, updated_at],
    )
    .expect("update hot should succeed");
    let _ = title; // unused here; kept for symmetry with other helpers
}

fn count_cold_catalog(db: &Path) -> i64 {
    Connection::open(db)
        .expect("db should open")
        .query_row("SELECT COUNT(*) FROM cold_catalog", [], |row| row.get(0))
        .expect("count should succeed")
}

fn count_knot_hot(db: &Path) -> i64 {
    Connection::open(db)
        .expect("db should open")
        .query_row("SELECT COUNT(*) FROM knot_hot", [], |row| row.get(0))
        .expect("count should succeed")
}

fn find_check<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["checks"]
        .as_array()
        .expect("checks should be an array")
        .iter()
        .find(|check| check["name"] == name)
        .unwrap_or_else(|| panic!("doctor report should contain check {name}"))
}

#[test]
fn doctor_passes_when_cold_holds_only_old_terminal_knots() {
    // The exact configuration the user reported as a permanent warn: a small
    // hot tier and a non-empty cold tier of legitimately-old terminal knots.
    // Under the invariant-based check this is healthy steady state.
    let root_ws = unique_workspace("knots-cli-cold-tier-healthy");
    let root = root_ws.path().to_path_buf();
    setup_repo_with_remote(&root);
    let db = root.join(".knots/cache/state.sqlite");
    bootstrap_workflows(&root, &db);

    let hot_id = create_knot(&root, &db, "Hot one");
    let cold_a = create_knot(&root, &db, "Old shipped A");
    let cold_b = create_knot(&root, &db, "Old shipped B");
    demote_to_cold_terminal(&db, &cold_a, "Old shipped A", "2024-01-01T00:00:00Z");
    demote_to_cold_terminal(&db, &cold_b, "Old shipped B", "2024-02-01T00:00:00Z");

    let doctor = run_knots(&root, &db, &["doctor", "--json"]);
    assert_success(&doctor);
    let report: Value = serde_json::from_slice(&doctor.stdout).expect("doctor json should parse");
    let check = find_check(&report, "cold_tier_imbalance");
    assert_eq!(
        check["status"], "pass",
        "old terminal knots in cold are healthy steady state, got: {}",
        check["detail"]
    );
    let data = check
        .get("data")
        .expect("cold_tier_imbalance should carry data");
    assert_eq!(data["cold_count"], 2);
    assert_eq!(data["shadow"], 0);
    assert_eq!(data["non_terminal_cold"], 0);
    assert_eq!(data["stale_terminal_hot"], 0);

    // Sanity: the hot knot is still reachable.
    let show = run_knots(&root, &db, &["show", &hot_id, "--json"]);
    assert_success(&show);
}

#[test]
fn doctor_fix_demotes_stale_terminal_hot_to_cold() {
    let root_ws = unique_workspace("knots-cli-cold-tier-stale-hot");
    let root = root_ws.path().to_path_buf();
    setup_repo_with_remote(&root);
    let db = root.join(".knots/cache/state.sqlite");
    bootstrap_workflows(&root, &db);

    let id = create_knot(&root, &db, "Stale shipped");
    insert_stale_terminal_hot(&db, &id, "Stale shipped", "2024-01-01T00:00:00Z");

    let before = run_knots(&root, &db, &["doctor", "--json"]);
    assert_success(&before);
    let before_report: Value =
        serde_json::from_slice(&before.stdout).expect("doctor json should parse");
    let before_check = find_check(&before_report, "cold_tier_imbalance");
    assert_eq!(before_check["status"], "warn");
    assert!(
        before_check["detail"]
            .as_str()
            .expect("detail")
            .contains("stale_terminal_hot=1"),
        "detail should surface stale-terminal hot count: {}",
        before_check["detail"]
    );

    let fix = run_knots(&root, &db, &["doctor", "--fix"]);
    assert_success(&fix);

    assert_eq!(count_knot_hot(&db), 0, "stale hot row demoted out");
    assert_eq!(count_cold_catalog(&db), 1, "demoted row landed in cold");

    let after = run_knots(&root, &db, &["doctor", "--json"]);
    assert_success(&after);
    let report: Value = serde_json::from_slice(&after.stdout).expect("doctor json should parse");
    let check = find_check(&report, "cold_tier_imbalance");
    assert_eq!(check["status"], "pass");
}

#[test]
fn doctor_fix_rehydrates_recent_terminal_cold_rows() {
    let root_ws = unique_workspace("knots-cli-cold-tier-recent-cold");
    let root = root_ws.path().to_path_buf();
    setup_repo_with_remote(&root);
    let db = root.join(".knots/cache/state.sqlite");
    bootstrap_workflows(&root, &db);

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Recent shipped",
            "--profile",
            "default",
            "--state",
            "shipped",
        ],
    );
    assert_success(&created);
    let id = resolve_full_id(&root, &db, &parse_created_id(&created));
    let recent = fmt_rfc3339(OffsetDateTime::now_utc() - Duration::hours(1));
    demote_to_cold_terminal(&db, &id, "Recent shipped", &recent);

    let before = run_knots(&root, &db, &["doctor", "--json"]);
    assert_success(&before);
    let before_report: Value =
        serde_json::from_slice(&before.stdout).expect("doctor json should parse");
    let before_check = find_check(&before_report, "cold_tier_imbalance");
    assert_eq!(before_check["status"], "warn");
    assert!(before_check["detail"]
        .as_str()
        .expect("detail")
        .contains("recent_terminal_cold=1"));

    let fix = run_knots(&root, &db, &["doctor", "--fix"]);
    assert_success(&fix);

    assert_eq!(count_knot_hot(&db), 1, "recent cold row should rehydrate");
    assert_eq!(
        count_cold_catalog(&db),
        0,
        "recent cold row should leave cold"
    );
    let listed = run_knots(&root, &db, &["ls", "-a", "--query", &id, "--json"]);
    assert_success(&listed);
    let rows: Value = serde_json::from_slice(&listed.stdout).expect("list json should parse");
    assert_eq!(rows[0]["id"], id);
}

fn insert_shadow_cold(db: &Path, id: &str, title: &str, updated_at: &str) {
    // Leaves the hot row intact while inserting a cold_catalog row with the
    // same id — the data-consistency leftover that kept the imbalance
    // warning lit indefinitely before the fix.
    let conn = Connection::open(db).expect("db should open");
    conn.execute(
        "INSERT OR REPLACE INTO knot_warm (id, title) VALUES (?1, ?2)",
        rusqlite::params![id, title],
    )
    .expect("insert warm should succeed");
    conn.execute(
        "INSERT OR REPLACE INTO cold_catalog (id, title, state, updated_at) \
         VALUES (?1, ?2, 'implementation', ?3)",
        rusqlite::params![id, title, updated_at],
    )
    .expect("insert cold should succeed");
}

#[test]
fn doctor_fix_prunes_cold_row_shadowed_by_hot() {
    let root_ws = unique_workspace("knots-cli-cold-tier-shadow");
    let root = root_ws.path().to_path_buf();
    setup_repo_with_remote(&root);
    let db = root.join(".knots/cache/state.sqlite");
    bootstrap_workflows(&root, &db);

    let hot_id = create_knot(&root, &db, "Hot one");
    // The hot row still exists — simulate the data-consistency glitch where
    // a prior bug left a cold_catalog entry for the same id.
    insert_shadow_cold(&db, &hot_id, "Hot one", "2026-04-09T00:00:00Z");

    assert_eq!(count_knot_hot(&db), 1);
    assert_eq!(count_cold_catalog(&db), 1);

    let before = run_knots(&root, &db, &["doctor", "--json"]);
    assert_success(&before);
    let before_report: Value =
        serde_json::from_slice(&before.stdout).expect("doctor json should parse");
    let before_check = find_check(&before_report, "cold_tier_imbalance");
    assert_eq!(before_check["status"], "warn");
    let before_detail = before_check["detail"]
        .as_str()
        .expect("detail should be a string");
    assert!(
        before_detail.contains("shadow=1"),
        "pre-fix detail should report shadow count: {before_detail}"
    );

    let fix = run_knots(&root, &db, &["doctor", "--fix"]);
    assert_success(&fix);

    assert_eq!(
        count_knot_hot(&db),
        1,
        "pruning cold shadows must not touch hot rows"
    );
    assert_eq!(
        count_cold_catalog(&db),
        0,
        "shadowed cold row should be pruned"
    );

    let after = run_knots(&root, &db, &["doctor", "--json"]);
    assert_success(&after);
    let report: Value = serde_json::from_slice(&after.stdout).expect("doctor json should parse");
    let check = find_check(&report, "cold_tier_imbalance");
    assert_eq!(
        check["status"], "pass",
        "cold_tier_imbalance should clear after --fix"
    );
}
