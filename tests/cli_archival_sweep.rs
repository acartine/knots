//! End-to-end tests for the cold-tier archival behaviors.
//!
//! These exercise the behaviors from knot `8aba`:
//! - `kno ls` runs the sweep inline and prints a summary line when it moves
//!   knots to cold.
//! - `kno show <id>` falls back to `cold_catalog` on a hot miss.
//! - `kno rehydrate <id>` bumps `updated_at` so the knot is not immediately
//!   re-eligible for the next sweep.
//! - BLOCKED and DEFERRED (passive-waiting) are not swept.

mod cli_dispatch_helpers;

use cli_dispatch_helpers::*;

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::params;
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

fn bootstrap_via_init(root: &Path, db: &Path) {
    setup_repo(root);
    // Create one knot so the DB is fully bootstrapped (schema + defaults).
    assert_success(&run_knots(
        root,
        db,
        &["new", "bootstrap", "--state", "idea"],
    ));
}

fn fmt_rfc3339(ts: OffsetDateTime) -> String {
    ts.format(&Rfc3339).expect("format ts")
}

struct Seed<'a> {
    id: &'a str,
    title: &'a str,
    state: &'a str,
    updated_at: &'a str,
}

fn seed_knots(db: &Path, knots: &[Seed<'_>]) {
    let conn = rusqlite::Connection::open(db).expect("db open");
    for k in knots {
        conn.execute(
            r#"
INSERT INTO knot_hot (
    id, title, state, updated_at, body, description, acceptance,
    priority, knot_type, tags_json, notes_json,
    handoff_capsules_json, invariants_json, step_history_json,
    gate_data_json, lease_data_json, execution_plan_data_json, lease_id,
    workflow_id, profile_id, profile_etag,
    deferred_from_state, blocked_from_state, created_at
) VALUES (
    ?1, ?2, ?3, ?4, NULL, NULL, NULL,
    NULL, 'work', '[]', '[]',
    '[]', '[]', '[]',
    '{}', '{}', '{}', NULL,
    'work_sdlc', 'autopilot', NULL,
    NULL, NULL, ?4
)
"#,
            params![k.id, k.title, k.state, k.updated_at],
        )
        .expect("seed insert");
    }
}

fn count_hot(db: &Path) -> i64 {
    let conn = rusqlite::Connection::open(db).expect("db open");
    conn.query_row("SELECT COUNT(*) FROM knot_hot", [], |row| row.get(0))
        .expect("count hot")
}

fn count_cold(db: &Path) -> i64 {
    let conn = rusqlite::Connection::open(db).expect("db open");
    conn.query_row("SELECT COUNT(*) FROM cold_catalog", [], |row| row.get(0))
        .expect("count cold")
}

fn unique_id(tag: &str, idx: usize) -> String {
    // The UUID v7 timestamp prefix keeps sort order stable across runs.
    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros();
    format!("seed-{tag}-{micros}-{idx:04}")
}

#[test]
fn ls_sweeps_stale_terminals_and_prints_summary() {
    let root_ws = unique_workspace("knots-archival-sweep");
    let root = root_ws.path().to_path_buf();
    let db = root.join(".knots/cache/state.sqlite");
    bootstrap_via_init(&root, &db);

    let now = OffsetDateTime::now_utc();
    let fresh = fmt_rfc3339(now - Duration::hours(1));
    let stale = fmt_rfc3339(now - Duration::hours(96));

    let mut ids = Vec::new();
    let mut seeds: Vec<Seed> = Vec::new();
    let fresh_ids: Vec<String> = (0..100).map(|i| unique_id("hot", i)).collect();
    let term_ids: Vec<String> = (0..50).map(|i| unique_id("term", i)).collect();
    for id in &fresh_ids {
        ids.push(id.clone());
        seeds.push(Seed {
            id,
            title: "Fresh",
            state: "ready_for_planning",
            updated_at: &fresh,
        });
    }
    for id in &term_ids {
        ids.push(id.clone());
        seeds.push(Seed {
            id,
            title: "Shipped long ago",
            state: "shipped",
            updated_at: &stale,
        });
    }
    seed_knots(&db, &seeds);

    // 1 bootstrap + 100 fresh + 50 stale-terminal = 151 knots hot.
    assert_eq!(count_hot(&db), 151);

    let ls = run_knots(&root, &db, &["ls"]);
    assert_success(&ls);
    let stdout = String::from_utf8_lossy(&ls.stdout);
    assert!(
        stdout.contains("archived 50 knots to cold storage")
            || stdout.contains("archived 51 knots to cold storage"),
        "expected archive summary line, got: {stdout}"
    );
    // Hot should now be 101 (bootstrap knot + 100 fresh) or 100.
    let hot = count_hot(&db);
    assert!(
        (100..=101).contains(&hot),
        "expected hot near target, got {hot}"
    );
    assert!(count_cold(&db) >= 50);
}

#[test]
fn ls_does_not_sweep_recent_terminals() {
    let root_ws = unique_workspace("knots-archival-recent");
    let root = root_ws.path().to_path_buf();
    let db = root.join(".knots/cache/state.sqlite");
    bootstrap_via_init(&root, &db);

    let now = OffsetDateTime::now_utc();
    let fresh = fmt_rfc3339(now - Duration::hours(1));
    let recent_term = fmt_rfc3339(now - Duration::hours(10));

    let fresh_ids: Vec<String> = (0..105).map(|i| unique_id("hot", i)).collect();
    let term_ids: Vec<String> = (0..10).map(|i| unique_id("term", i)).collect();
    let mut seeds: Vec<Seed> = Vec::new();
    for id in &fresh_ids {
        seeds.push(Seed {
            id,
            title: "Fresh",
            state: "ready_for_planning",
            updated_at: &fresh,
        });
    }
    for id in &term_ids {
        seeds.push(Seed {
            id,
            title: "Just shipped",
            state: "shipped",
            updated_at: &recent_term,
        });
    }
    seed_knots(&db, &seeds);

    // 1 bootstrap + 105 fresh + 10 recent-term = 116 knots; above HWM,
    // but nothing eligible.
    let hot_before = count_hot(&db);
    assert_eq!(hot_before, 116);
    let ls = run_knots(&root, &db, &["ls"]);
    assert_success(&ls);
    let stdout = String::from_utf8_lossy(&ls.stdout);
    assert!(
        !stdout.contains("archived"),
        "no archive expected, got: {stdout}"
    );
    assert_eq!(count_hot(&db), hot_before);
    assert_eq!(count_cold(&db), 0);
}

#[test]
fn ls_skips_blocked_and_deferred_even_when_stale() {
    let root_ws = unique_workspace("knots-archival-blocked");
    let root = root_ws.path().to_path_buf();
    let db = root.join(".knots/cache/state.sqlite");
    bootstrap_via_init(&root, &db);

    let now = OffsetDateTime::now_utc();
    let fresh = fmt_rfc3339(now - Duration::hours(1));
    let stale = fmt_rfc3339(now - Duration::hours(200));

    let fresh_ids: Vec<String> = (0..100).map(|i| unique_id("hot", i)).collect();
    let blocked_ids: Vec<String> = (0..10).map(|i| unique_id("blk", i)).collect();
    let deferred_ids: Vec<String> = (0..5).map(|i| unique_id("def", i)).collect();
    let mut seeds: Vec<Seed> = Vec::new();
    for id in &fresh_ids {
        seeds.push(Seed {
            id,
            title: "Fresh",
            state: "ready_for_planning",
            updated_at: &fresh,
        });
    }
    for id in &blocked_ids {
        seeds.push(Seed {
            id,
            title: "Blocked",
            state: "blocked",
            updated_at: &stale,
        });
    }
    for id in &deferred_ids {
        seeds.push(Seed {
            id,
            title: "Deferred",
            state: "deferred",
            updated_at: &stale,
        });
    }
    seed_knots(&db, &seeds);

    // 1 bootstrap + 100 fresh + 10 blocked + 5 deferred = 116, over HWM
    // but none terminal.
    assert_eq!(count_hot(&db), 116);
    let ls = run_knots(&root, &db, &["ls"]);
    assert_success(&ls);
    let stdout = String::from_utf8_lossy(&ls.stdout);
    assert!(
        !stdout.contains("archived"),
        "blocked/deferred are not terminal; expected no sweep: {stdout}"
    );
    assert_eq!(count_hot(&db), 116);
    assert_eq!(count_cold(&db), 0);
}

#[test]
fn show_reads_cold_catalog_after_sweep() {
    let root_ws = unique_workspace("knots-archival-show-cold");
    let root = root_ws.path().to_path_buf();
    let db = root.join(".knots/cache/state.sqlite");
    bootstrap_via_init(&root, &db);

    let now = OffsetDateTime::now_utc();
    let fresh = fmt_rfc3339(now - Duration::hours(1));
    let stale = fmt_rfc3339(now - Duration::hours(120));

    let target_id = format!("archive-target-{}", now.unix_timestamp());
    let fresh_ids: Vec<String> = (0..100).map(|i| unique_id("hot", i)).collect();
    let term_ids: Vec<String> = (0..20).map(|i| unique_id("term", i)).collect();
    let mut seeds: Vec<Seed> = Vec::new();
    seeds.push(Seed {
        id: &target_id,
        title: "Target shipped knot",
        state: "shipped",
        updated_at: &stale,
    });
    for id in &fresh_ids {
        seeds.push(Seed {
            id,
            title: "Fresh",
            state: "ready_for_planning",
            updated_at: &fresh,
        });
    }
    for id in &term_ids {
        seeds.push(Seed {
            id,
            title: "Shipped",
            state: "shipped",
            updated_at: &stale,
        });
    }
    seed_knots(&db, &seeds);

    // Run ls to trigger the sweep.
    let ls = run_knots(&root, &db, &["ls"]);
    assert_success(&ls);

    // Now show the target — should resolve via cold catalog.
    let show = run_knots(&root, &db, &["show", &target_id, "--json"]);
    assert_success(&show);
    let value: serde_json::Value = serde_json::from_slice(&show.stdout).expect("show json");
    assert_eq!(value["id"].as_str(), Some(target_id.as_str()));
    assert_eq!(value["state"].as_str(), Some("shipped"));
    assert_eq!(value["title"].as_str(), Some("Target shipped knot"));
}

#[test]
fn rehydrate_bumps_updated_at_and_is_not_immediately_re_swept() {
    let root_ws = unique_workspace("knots-archival-rehydrate");
    let root = root_ws.path().to_path_buf();
    let db = root.join(".knots/cache/state.sqlite");
    bootstrap_via_init(&root, &db);

    let now = OffsetDateTime::now_utc();
    let stale = fmt_rfc3339(now - Duration::hours(200));

    // Seed a knot directly into cold_catalog, as if it had been archived.
    let conn = rusqlite::Connection::open(&db).expect("db open");
    let cold_id = format!("cold-reh-{}", now.unix_timestamp());
    conn.execute(
        "INSERT INTO cold_catalog (id, title, state, updated_at) \
         VALUES (?1, ?2, ?3, ?4)",
        params![cold_id.as_str(), "Was shipped long ago", "shipped", &stale],
    )
    .expect("seed cold");
    // Provide a minimal index event so rehydrate_from_events has what it
    // needs. Write it directly.
    let idx_dir = root.join(".knots/index/2026/01/01");
    std::fs::create_dir_all(&idx_dir).expect("mk idx dir");
    let event = serde_json::json!({
        "event_id": "evt-1",
        "occurred_at": stale,
        "type": "idx.knot_head",
        "data": {
            "knot_id": cold_id,
            "title": "Was shipped long ago",
            "state": "shipped",
            "workflow_id": "work_sdlc",
            "profile_id": "autopilot",
            "updated_at": stale,
            "terminal": true,
        },
    });
    std::fs::write(
        idx_dir.join("0001-idx.knot_head.json"),
        serde_json::to_string(&event).unwrap(),
    )
    .expect("write event");

    drop(conn);

    // Rehydrate the knot.
    let rehydrate = run_knots(&root, &db, &["rehydrate", &cold_id]);
    assert_success(&rehydrate);

    // Observe that the knot is now in knot_hot with a recent updated_at.
    let conn = rusqlite::Connection::open(&db).expect("db open");
    let row: (String, String) = conn
        .query_row(
            "SELECT state, updated_at FROM knot_hot WHERE id = ?1",
            params![cold_id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("rehydrated row");
    assert_eq!(row.0, "shipped");
    let rehydrated_at = OffsetDateTime::parse(&row.1, &Rfc3339).expect("rfc3339 updated_at");
    let delta = now - rehydrated_at;
    assert!(
        delta.whole_seconds().abs() < 120,
        "updated_at should be bumped to ~now, got {} (delta {:?})",
        row.1,
        delta
    );
    // cold_catalog should no longer contain the id.
    let cold_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM cold_catalog WHERE id = ?1",
            params![cold_id.as_str()],
            |r| r.get(0),
        )
        .expect("count cold by id");
    assert_eq!(cold_exists, 0);
}
