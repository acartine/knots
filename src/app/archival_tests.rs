use std::path::PathBuf;

use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::super::App;
use super::{ColdSweepReport, HOT_HIGH_WATER, HOT_TARGET};
use crate::db;
use crate::domain::execution_plan::ExecutionPlanData;
use crate::domain::gate::GateData;
use crate::domain::lease::LeaseData;

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-archival-test-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("temp workspace should be creatable");
    root
}

fn new_app() -> (App, PathBuf) {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app =
        App::open(db_path.to_str().expect("utf8 path"), root.clone()).expect("app should open");
    (app, root)
}

fn fmt(ts: OffsetDateTime) -> String {
    ts.format(&Rfc3339).expect("format")
}

struct SeedKnot<'a> {
    id: &'a str,
    state: &'a str,
    updated_at: &'a str,
}

fn seed(app: &App, knots: &[SeedKnot<'_>]) {
    let gate = GateData::default();
    let lease = LeaseData::default();
    let plan = ExecutionPlanData::default();
    let now = fmt(OffsetDateTime::now_utc());
    for k in knots {
        db::upsert_knot_hot(
            app.conn_for_test(),
            &db::UpsertKnotHot {
                id: k.id,
                title: "seed",
                state: k.state,
                updated_at: k.updated_at,
                body: None,
                description: None,
                acceptance: None,
                priority: None,
                knot_type: Some("work"),
                tags: &[],
                notes: &[],
                handoff_capsules: &[],
                invariants: &[],
                verification_steps: &[],
                step_history: &[],
                gate_data: &gate,
                lease_data: &lease,
                execution_plan_data: &plan,
                lease_id: None,
                lease_expiry_ts: 0,
                workflow_id: "work_sdlc",
                profile_id: "autopilot",
                profile_etag: None,
                deferred_from_state: None,
                blocked_from_state: None,
                created_at: Some(&now),
            },
        )
        .expect("seed insert");
    }
}

fn count_hot(app: &App) -> i64 {
    app.conn_for_test()
        .query_row("SELECT COUNT(*) FROM knot_hot", [], |row| row.get(0))
        .expect("count")
}

fn count_cold(app: &App) -> i64 {
    app.conn_for_test()
        .query_row("SELECT COUNT(*) FROM cold_catalog", [], |row| row.get(0))
        .expect("count")
}

#[test]
fn sweep_noop_below_high_water() {
    let (app, root) = new_app();
    let stale = fmt(OffsetDateTime::now_utc() - Duration::hours(200));
    let mut items: Vec<(String, String, String)> = Vec::new();
    for i in 0..50 {
        items.push((format!("k-a-{i:04}"), "shipped".to_string(), stale.clone()));
    }
    let seeds: Vec<SeedKnot> = items
        .iter()
        .map(|(id, s, t)| SeedKnot {
            id,
            state: s,
            updated_at: t,
        })
        .collect();
    seed(&app, &seeds);

    let report = app.run_cold_sweep().expect("sweep");
    assert!(report.is_empty());
    assert_eq!(count_hot(&app), 50);
    assert_eq!(count_cold(&app), 0);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn sweep_moves_stale_terminal_down_to_target() {
    let (app, root) = new_app();
    let now = OffsetDateTime::now_utc();
    let stale = fmt(now - Duration::hours(96));
    let fresh = fmt(now - Duration::hours(1));
    let mut items: Vec<(String, &str, String)> = Vec::new();
    for i in 0..100 {
        items.push((format!("k-hot-{i:04}"), "ready_for_planning", fresh.clone()));
    }
    for i in 0..50 {
        items.push((format!("k-term-{i:04}"), "shipped", stale.clone()));
    }
    let seeds: Vec<SeedKnot> = items
        .iter()
        .map(|(id, s, t)| SeedKnot {
            id,
            state: s,
            updated_at: t,
        })
        .collect();
    seed(&app, &seeds);

    assert_eq!(count_hot(&app), 150);
    let report = app.run_cold_sweep().expect("sweep");
    assert_eq!(report.len(), 50);
    assert_eq!(count_hot(&app), HOT_TARGET as i64);
    assert_eq!(count_cold(&app), 50);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn sweep_skips_recent_terminal() {
    let (app, root) = new_app();
    let now = OffsetDateTime::now_utc();
    let recent_term = fmt(now - Duration::hours(10));
    let fresh = fmt(now - Duration::hours(1));
    let mut items: Vec<(String, &str, String)> = Vec::new();
    for i in 0..100 {
        items.push((format!("k-hot-{i:04}"), "ready_for_planning", fresh.clone()));
    }
    for i in 0..15 {
        items.push((format!("k-term-{i:04}"), "shipped", recent_term.clone()));
    }
    let seeds: Vec<SeedKnot> = items
        .iter()
        .map(|(id, s, t)| SeedKnot {
            id,
            state: s,
            updated_at: t,
        })
        .collect();
    seed(&app, &seeds);

    assert_eq!(count_hot(&app), 115);
    let report = app.run_cold_sweep().expect("sweep");
    assert!(report.is_empty(), "recent terminals must not be swept");
    assert_eq!(count_hot(&app), 115);
    assert_eq!(count_cold(&app), 0);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn sweep_does_not_touch_blocked_or_deferred() {
    let (app, root) = new_app();
    let now = OffsetDateTime::now_utc();
    let stale = fmt(now - Duration::hours(200));
    let fresh = fmt(now - Duration::hours(1));
    let mut items: Vec<(String, &str, String)> = Vec::new();
    for i in 0..100 {
        items.push((format!("k-hot-{i:04}"), "ready_for_planning", fresh.clone()));
    }
    for i in 0..10 {
        items.push((format!("k-blk-{i:04}"), "blocked", stale.clone()));
    }
    for i in 0..5 {
        items.push((format!("k-def-{i:04}"), "deferred", stale.clone()));
    }
    let seeds: Vec<SeedKnot> = items
        .iter()
        .map(|(id, s, t)| SeedKnot {
            id,
            state: s,
            updated_at: t,
        })
        .collect();
    seed(&app, &seeds);

    assert_eq!(count_hot(&app), 115);
    let report: ColdSweepReport = app.run_cold_sweep().expect("sweep");
    assert!(report.is_empty());
    assert_eq!(count_hot(&app), 115);
    assert_eq!(count_cold(&app), 0);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn sweep_partial_when_fewer_eligible_than_excess() {
    let (app, root) = new_app();
    let now = OffsetDateTime::now_utc();
    let stale = fmt(now - Duration::hours(200));
    let fresh = fmt(now - Duration::hours(1));
    let mut items: Vec<(String, &str, String)> = Vec::new();
    for i in 0..120 {
        items.push((format!("k-hot-{i:04}"), "ready_for_planning", fresh.clone()));
    }
    // Only 10 stale-terminal knots, but excess above target is 30+.
    for i in 0..10 {
        items.push((format!("k-term-{i:04}"), "shipped", stale.clone()));
    }
    let seeds: Vec<SeedKnot> = items
        .iter()
        .map(|(id, s, t)| SeedKnot {
            id,
            state: s,
            updated_at: t,
        })
        .collect();
    seed(&app, &seeds);

    assert_eq!(count_hot(&app), 130);
    let report = app.run_cold_sweep().expect("sweep");
    assert_eq!(report.len(), 10);
    assert_eq!(count_hot(&app), 120);
    assert_eq!(count_cold(&app), 10);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn sweep_moves_oldest_first() {
    // Seed 100 fresh non-terminal + 20 stale terminals. Excess above target is
    // 20 but we add extra room by seeding extra fresh to force a limit smaller
    // than the eligible pool:
    // Hot = 115 (100 fresh non-terminal + 15 stale-terminal) → excess = 15.
    // But we want to test oldest-first ordering, so seed 20 terminals and
    // give the sweep a limit of 15 (excess = 20 - 5 = 15).
    let (app, root) = new_app();
    let now = OffsetDateTime::now_utc();
    let fresh = fmt(now - Duration::hours(1));
    // 95 fresh non-terminal + 20 stale terminals → hot = 115, excess above
    // target = 15. Terminal indices 5..19 (the 15 oldest) should sweep; the
    // 5 newest-stale terminals should remain hot.
    let mut items: Vec<(String, &str, String)> = Vec::new();
    for i in 0..95 {
        items.push((format!("k-hot-{i:04}"), "ready_for_planning", fresh.clone()));
    }
    let mut term_ids: Vec<(String, i64)> = Vec::new();
    for i in 0i64..20 {
        // Age = 100 + i hours → index 19 is the oldest (most-stale).
        let ts = fmt(now - Duration::hours(100 + i));
        let id = format!("k-term-{i:04}");
        term_ids.push((id.clone(), i));
        items.push((id, "shipped", ts));
    }
    let seeds: Vec<SeedKnot> = items
        .iter()
        .map(|(id, s, t)| SeedKnot {
            id,
            state: s,
            updated_at: t,
        })
        .collect();
    seed(&app, &seeds);

    // Hot count = 115 → excess above target = 15 → sweep should move 15
    // oldest terminals (indices 5..19).
    assert_eq!(count_hot(&app), 115);
    let report = app.run_cold_sweep().expect("sweep");
    assert_eq!(report.len(), 15);

    for (id, idx) in &term_ids {
        let in_cold = db::get_cold_catalog(app.conn_for_test(), id)
            .expect("lookup")
            .is_some();
        if *idx >= 5 {
            assert!(in_cold, "{id} (idx {idx}) should have been swept");
        } else {
            assert!(!in_cold, "{id} (idx {idx}) should remain hot");
        }
    }

    let _ = std::fs::remove_dir_all(root);
}

const _: () = assert!(HOT_HIGH_WATER > HOT_TARGET);

#[test]
fn show_knot_falls_back_to_cold_catalog() {
    let (app, root) = new_app();
    let updated_at = fmt(OffsetDateTime::now_utc() - Duration::hours(200));
    db::upsert_cold_catalog(
        app.conn_for_test(),
        "cold-xyz",
        "Cold Knot Title",
        "shipped",
        &updated_at,
    )
    .expect("seed cold");

    let view = app
        .show_knot("cold-xyz")
        .expect("show")
        .expect("should resolve via cold catalog");
    assert_eq!(view.id, "cold-xyz");
    assert_eq!(view.title, "Cold Knot Title");
    assert_eq!(view.state, "shipped");
    assert_eq!(view.updated_at, updated_at);
    // Full body is not populated on cold views.
    assert!(view.body.is_none());
    assert!(view.edges.is_empty());
    assert!(view.child_summaries.is_empty());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn show_knot_returns_none_when_neither_hot_nor_cold() {
    let (app, root) = new_app();
    let result = app
        .show_knot("nonexistent-knot-id-0000")
        .expect("show should not error");
    assert!(result.is_none());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn list_knots_stores_sweep_report_for_caller() {
    let (app, root) = new_app();
    let now = OffsetDateTime::now_utc();
    let stale = fmt(now - Duration::hours(200));
    let fresh = fmt(now - Duration::hours(1));
    let mut items: Vec<(String, &str, String)> = Vec::new();
    for i in 0..100 {
        items.push((format!("k-hot-{i:04}"), "ready_for_planning", fresh.clone()));
    }
    for i in 0..15 {
        items.push((format!("k-term-{i:04}"), "shipped", stale.clone()));
    }
    let seeds: Vec<SeedKnot> = items
        .iter()
        .map(|(id, s, t)| SeedKnot {
            id,
            state: s,
            updated_at: t,
        })
        .collect();
    seed(&app, &seeds);

    // Initially, no report.
    assert!(app.take_cold_sweep_report().is_none());

    // list_knots triggers sweep and populates the report.
    let knots = app.list_knots().expect("list");
    assert_eq!(knots.len() as i64, count_hot(&app));
    let report = app
        .take_cold_sweep_report()
        .expect("sweep report should be recorded");
    assert_eq!(report.len(), 15);

    // Calling take again returns None — report is consumed.
    assert!(app.take_cold_sweep_report().is_none());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn sweep_with_kno_trace_env_exercises_trace_branch() {
    let (app, root) = new_app();
    let now = OffsetDateTime::now_utc();
    let stale = fmt(now - Duration::hours(96));
    let fresh = fmt(now - Duration::hours(1));
    let mut items: Vec<(String, &str, String)> = Vec::new();
    for i in 0..100 {
        items.push((format!("k-hot-{i:04}"), "ready_for_planning", fresh.clone()));
    }
    for i in 0..15 {
        items.push((format!("k-term-{i:04}"), "shipped", stale.clone()));
    }
    let seeds: Vec<SeedKnot> = items
        .iter()
        .map(|(id, s, t)| SeedKnot {
            id,
            state: s,
            updated_at: t,
        })
        .collect();
    seed(&app, &seeds);

    // Safety: env vars are process-global. We clean up after the sweep so
    // other tests aren't affected. Tests in this module run serially because
    // they all use distinct temp dirs, but env var state leaks across;
    // using a unique var name is not enough.
    // SAFETY: tests are run single-threaded by cargo when they manipulate env.
    // To be extra defensive, unset after.
    // SAFETY: env vars can be modified from a single-threaded context.
    unsafe {
        std::env::set_var("KNO_TRACE", "1");
    }
    let report = app.run_cold_sweep().expect("sweep");
    unsafe {
        std::env::remove_var("KNO_TRACE");
    }
    assert_eq!(report.len(), 15);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn list_knots_no_report_when_sweep_noop() {
    let (app, root) = new_app();
    let fresh = fmt(OffsetDateTime::now_utc() - Duration::hours(1));
    let mut items: Vec<(String, &str, String)> = Vec::new();
    for i in 0..50 {
        items.push((format!("k-hot-{i:04}"), "ready_for_planning", fresh.clone()));
    }
    let seeds: Vec<SeedKnot> = items
        .iter()
        .map(|(id, s, t)| SeedKnot {
            id,
            state: s,
            updated_at: t,
        })
        .collect();
    seed(&app, &seeds);

    let _ = app.list_knots().expect("list");
    assert!(app.take_cold_sweep_report().is_none());

    let _ = std::fs::remove_dir_all(root);
}
