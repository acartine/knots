use std::path::Path;

use serde_json::json;

use crate::db::{self, UpsertKnotHot};
use crate::domain::step_history::StepStatus;
use crate::events::{FullEvent, FullEventKind};
use crate::sync::GitAdapter;

use super::IncrementalApplier;

fn unique_workspace() -> knots_test_support::TestWorkspace {
    knots_test_support::workspace("knots-sync-step")
}

fn open_conn(root: &Path) -> rusqlite::Connection {
    let db_path = root.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(db_path.parent().expect("db parent should exist"))
        .expect("db parent should be creatable");
    db::open_connection(db_path.to_str().expect("utf8 db path")).expect("db should open")
}

fn seed_hot_knot(conn: &rusqlite::Connection, knot_id: &str) {
    db::upsert_knot_hot(
        conn,
        &UpsertKnotHot {
            id: knot_id,
            title: "Seed",
            state: "ready_for_planning",
            updated_at: "2026-02-25T10:00:00Z",
            body: None,
            description: None,
            acceptance: None,
            priority: None,
            knot_type: None,
            tags: &[],
            notes: &[],
            handoff_capsules: &[],
            invariants: &[],
            verification_steps: &[],
            step_history: &[],
            gate_data: &crate::domain::gate::GateData::default(),
            lease_data: &crate::domain::lease::LeaseData::default(),
            execution_plan_data: &crate::domain::execution_plan::ExecutionPlanData::default(),
            lease_id: None,
            lease_expiry_ts: 0,
            workflow_id: "knots_sdlc",
            profile_id: "automation_granular",
            profile_etag: Some("etag-1"),
            deferred_from_state: None,
            blocked_from_state: None,
            created_at: Some("2026-02-25T10:00:00Z"),
        },
    )
    .expect("hot knot should upsert");
}

#[test]
fn apply_full_event_state_set_replays_step_history() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let conn = open_conn(&root);
    seed_hot_knot(&conn, "K-STEP");
    let applier = IncrementalApplier::new_with_builtins(&conn, root.clone(), GitAdapter::new());

    let event = FullEvent::with_identity(
        "7000",
        "2026-02-25T10:03:00Z",
        "K-STEP",
        FullEventKind::KnotStateSet.as_str(),
        json!({
            "from": "ready_for_planning",
            "to": "planning",
            "actor_kind": "agent",
            "agent_name": "sandbox-probe",
            "agent_model": "sandbox-probe",
            "agent_version": "1.0.0"
        }),
    );
    let rel_path = Path::new(".knots/events/2026/02/25/7000-knot.state_set.json");
    let path = root.join(rel_path);
    std::fs::create_dir_all(path.parent().expect("event parent should exist"))
        .expect("event directory should be creatable");
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&event).expect("event should serialize"),
    )
    .expect("event should write");

    applier
        .apply_full_event(rel_path)
        .expect("state_set event should apply");

    let updated = db::get_knot_hot(&conn, "K-STEP")
        .expect("hot lookup should succeed")
        .expect("hot knot should remain present");
    assert_eq!(updated.state, "planning");
    let [record] = updated.step_history.as_slice() else {
        panic!("expected one replayed step record");
    };
    assert_eq!(record.step, "planning");
    assert_eq!(record.from_state, "ready_for_planning");
    assert_eq!(record.status, StepStatus::Started);
    assert_eq!(record.actor_kind.as_deref(), Some("agent"));
    assert_eq!(record.agent_name.as_deref(), Some("sandbox-probe"));
    assert_eq!(record.agent_model.as_deref(), Some("sandbox-probe"));
    assert_eq!(record.agent_version.as_deref(), Some("1.0.0"));
}
