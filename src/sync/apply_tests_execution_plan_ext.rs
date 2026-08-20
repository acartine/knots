use std::process::Command;

use crate::db::{self, UpsertKnotHot};
use crate::domain::execution_plan::{ExecutionPlanData, ExecutionPlanStep, ExecutionPlanWave};
use crate::domain::gate::GateData;
use crate::domain::lease::LeaseData;
use crate::sync::GitAdapter;

use super::IncrementalApplier;

fn unique_workspace() -> knots_test_support::TestWorkspace {
    knots_test_support::workspace("knots-sync-plan-ext")
}

fn run_git(root: &std::path::Path, args: &[&str]) {
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
    std::fs::write(root.join("README.md"), "# apply\n").expect("readme should be writable");
    run_git(&root, &["add", "README.md"]);
    run_git(&root, &["commit", "-m", "init"]);
    root_ws
}

fn open_conn(root: &std::path::Path) -> rusqlite::Connection {
    let db_path = root.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(db_path.parent().expect("db parent should exist"))
        .expect("db parent should be creatable");
    db::open_connection(db_path.to_str().expect("utf8 db path")).expect("db should open")
}

fn seed_plan_with_waves(
    conn: &rusqlite::Connection,
    knot_id: &str,
    updated_at: &str,
    profile_etag: &str,
    waves: &[ExecutionPlanWave],
) {
    let plan_data = ExecutionPlanData {
        objective: Some("Seeded plan".to_string()),
        waves: waves.to_vec(),
        ..Default::default()
    };
    db::upsert_knot_hot(
        conn,
        &UpsertKnotHot {
            id: knot_id,
            title: "Seeded plan",
            state: "orchestration",
            updated_at,
            body: None,
            description: None,
            acceptance: None,
            priority: None,
            knot_type: Some("execution_plan"),
            tags: &[],
            notes: &[],
            handoff_capsules: &[],
            invariants: &[],
            verification_steps: &[],
            step_history: &[],
            gate_data: &GateData::default(),
            lease_data: &LeaseData::default(),
            execution_plan_data: &plan_data,
            lease_id: None,
            lease_expiry_ts: 0,
            workflow_id: "execution_plan_sdlc",
            profile_id: "autopilot",
            profile_etag: Some(profile_etag),
            deferred_from_state: None,
            blocked_from_state: None,
            created_at: Some(updated_at),
        },
    )
    .expect("seed plan should upsert");
}

#[test]
fn full_event_with_stale_precondition_is_ignored_and_preserves_existing_waves() {
    let root_ws = setup_repo();
    let root = root_ws.path().to_path_buf();
    let conn = open_conn(&root);
    db::set_meta(&conn, "hot_window_days", "365").expect("hot window should be configurable");

    let existing_wave = ExecutionPlanWave {
        wave_index: 3,
        name: "Preserved".to_string(),
        objective: "Keep this wave".to_string(),
        steps: vec![ExecutionPlanStep {
            step_index: 1,
            knot_ids: vec!["K-preserved".to_string()],
            notes: None,
        }],
        ..Default::default()
    };
    let current_updated_at = "2026-05-25T10:02:00Z";
    seed_plan_with_waves(
        &conn,
        "K-stale-plan",
        current_updated_at,
        "current-etag",
        &[existing_wave],
    );

    let events_dir = root.join(".knots/events/2026/05/25");
    std::fs::create_dir_all(&events_dir).expect("events directory should be creatable");
    let event_occurred_at = "2026-05-25T10:01:00Z";
    std::fs::write(
        events_dir.join("9000-knot.execution_plan_data_set.json"),
        serde_json::json!({
            "event_id": "9000",
            "occurred_at": event_occurred_at,
            "knot_id": "K-stale-plan",
            "type": "knot.execution_plan_data_set",
            "precondition": { "profile_etag": "old-etag" },
            "data": {
                "execution_plan": {
                    "objective": "Stale replacement attempt",
                    "waves": []
                }
            }
        })
        .to_string(),
    )
    .expect("stale full event should write");

    let mut applier = IncrementalApplier::new_with_builtins(&conn, root.clone(), GitAdapter::new());
    applier
        .apply_to_head("HEAD")
        .expect("apply_to_head should succeed even with stale event");

    let record = db::get_knot_hot(&conn, "K-stale-plan")
        .expect("hot lookup should succeed")
        .expect("record should still exist");
    assert_eq!(
        record.execution_plan_data.waves.len(),
        1,
        "stale full event must not wipe existing waves"
    );
    assert_eq!(record.execution_plan_data.waves[0].wave_index, 3);
}
