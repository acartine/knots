//! Regression tests for cold-sweep transaction atomicity.

use std::path::PathBuf;

use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::super::App;
use crate::db;
use crate::domain::execution_plan::ExecutionPlanData;
use crate::domain::gate::GateData;
use crate::domain::lease::LeaseData;

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-archival-tx-test-{}", Uuid::now_v7()));
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

fn seed_hot(app: &App, id: &str, state: &str, updated_at: &str) {
    let gate = GateData::default();
    let lease = LeaseData::default();
    let plan = ExecutionPlanData::default();
    let now = fmt(OffsetDateTime::now_utc());
    db::upsert_knot_hot(
        app.conn_for_test(),
        &db::UpsertKnotHot {
            id,
            title: "seed",
            state,
            updated_at,
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
fn sweep_rolls_back_all_moves_when_any_insert_fails() {
    // Regression: move_candidates must commit atomically. If the second
    // upsert_cold_catalog fails, the first move must be rolled back so the
    // cold catalog stays empty and the hot rows stay in place.
    let (app, root) = new_app();
    let now = OffsetDateTime::now_utc();
    let stale = fmt(now - Duration::hours(200));
    let fresh = fmt(now - Duration::hours(1));
    for i in 0..100 {
        seed_hot(&app, &format!("k-hot-{i:04}"), "ready_for_planning", &fresh);
    }
    for i in 0..12 {
        seed_hot(&app, &format!("k-term-{i:04}"), "shipped", &stale);
    }
    assert_eq!(count_hot(&app), 112);

    // Install a trigger that aborts any insert of the sentinel id into
    // cold_catalog. Candidates are swept oldest-first; with identical stale
    // timestamps, ORDER BY updated_at, id ASC makes k-term-0000 the first
    // to move and k-term-0001 the second. Targeting k-term-0001 ensures the
    // first move succeeds and the second raises, exercising rollback of the
    // already-inserted cold row.
    app.conn_for_test()
        .execute_batch(
            "CREATE TRIGGER abort_sentinel_insert \
             BEFORE INSERT ON cold_catalog \
             WHEN NEW.id = 'k-term-0001' \
             BEGIN SELECT RAISE(ABORT, 'injected fault'); END;",
        )
        .expect("install trigger");

    let result = app.run_cold_sweep();
    assert!(result.is_err(), "sweep must surface the injected fault");

    assert_eq!(
        count_cold(&app),
        0,
        "no cold rows should remain committed after rollback",
    );
    assert_eq!(
        count_hot(&app),
        112,
        "no hot rows should have been deleted after rollback",
    );

    // Drop the trigger so subsequent operations (if any) are unaffected.
    app.conn_for_test()
        .execute_batch("DROP TRIGGER abort_sentinel_insert;")
        .expect("drop trigger");

    let _ = std::fs::remove_dir_all(root);
}
