//! Quarantine table CRUD and the local-leased knot-id set.

use super::unique_db_path;
use crate::db::{
    list_quarantined_knots, local_leased_knot_ids, open_connection, quarantine_knot_if_absent,
    remove_quarantine, update_lease_expiry_ts, upsert_knot_hot, UpsertKnotHot,
};
use crate::domain::lease::{LeaseData, LeaseOwner};
use crate::lease_expiry::compute_expiry_ts;

fn owned_by(machine_id: &str) -> LeaseData {
    LeaseData {
        nickname: "held".to_string(),
        owner: Some(LeaseOwner {
            machine_id: machine_id.to_string(),
            pid: 4242,
        }),
        ..LeaseData::default()
    }
}

fn insert_active_lease(conn: &rusqlite::Connection, id: &str, lease_data: &LeaseData) {
    upsert_knot_hot(
        conn,
        &UpsertKnotHot {
            id,
            title: id,
            state: "lease_active",
            updated_at: "2026-03-12T00:00:00Z",
            body: None,
            description: None,
            acceptance: None,
            priority: None,
            knot_type: Some("lease"),
            tags: &[],
            notes: &[],
            handoff_capsules: &[],
            invariants: &[],
            verification_steps: &[],
            step_history: &[],
            gate_data: &crate::domain::gate::GateData::default(),
            lease_data,
            execution_plan_data: &crate::domain::execution_plan::ExecutionPlanData::default(),
            lease_id: None,
            lease_expiry_ts: 0,
            workflow_id: "lease_sdlc",
            profile_id: "autopilot",
            profile_etag: None,
            deferred_from_state: None,
            blocked_from_state: None,
            created_at: None,
        },
    )
    .expect("lease upsert should succeed");
    update_lease_expiry_ts(conn, id, compute_expiry_ts(600)).expect("expiry should be settable");
}

fn insert_bound_work_knot(conn: &rusqlite::Connection, id: &str, lease_id: &str) {
    upsert_knot_hot(
        conn,
        &UpsertKnotHot {
            id,
            title: id,
            state: "implementation",
            updated_at: "2026-03-12T00:00:00Z",
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
            gate_data: &crate::domain::gate::GateData::default(),
            lease_data: &LeaseData::default(),
            execution_plan_data: &crate::domain::execution_plan::ExecutionPlanData::default(),
            lease_id: Some(lease_id),
            lease_expiry_ts: 0,
            workflow_id: "work_sdlc",
            profile_id: "autopilot",
            profile_etag: None,
            deferred_from_state: None,
            blocked_from_state: None,
            created_at: None,
        },
    )
    .expect("work knot upsert should succeed");
}

#[test]
fn quarantine_row_persists_and_can_be_removed() {
    let (_db_ws, path) = unique_db_path();
    let conn = open_connection(&path).expect("connection should open");

    quarantine_knot_if_absent(&conn, "K-1", "commit-a").expect("quarantine insert should succeed");
    let rows = list_quarantined_knots(&conn).expect("list should succeed");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].knot_id, "K-1");
    assert_eq!(rows[0].base_commit, "commit-a");

    remove_quarantine(&conn, "K-1").expect("remove should succeed");
    assert!(list_quarantined_knots(&conn)
        .expect("list should succeed")
        .is_empty());
}

#[test]
fn quarantine_records_the_base_commit_once() {
    let (_db_ws, path) = unique_db_path();
    let conn = open_connection(&path).expect("connection should open");

    quarantine_knot_if_absent(&conn, "K-1", "commit-a").expect("first skip should insert");
    quarantine_knot_if_absent(&conn, "K-1", "commit-b").expect("second skip should be a no-op");

    let rows = list_quarantined_knots(&conn).expect("list should succeed");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].base_commit, "commit-a",
        "the earliest base commit must survive repeated skips, or events between \
         commit-a and commit-b would never be replayed"
    );
}

#[test]
fn quarantine_rows_persist_across_a_reopened_connection() {
    let (_db_ws, path) = unique_db_path();
    {
        let conn = open_connection(&path).expect("connection should open");
        quarantine_knot_if_absent(&conn, "K-1", "commit-a")
            .expect("quarantine insert should succeed");
    }
    let conn = open_connection(&path).expect("reconnection should open");
    let rows = list_quarantined_knots(&conn).expect("list should succeed");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].knot_id, "K-1");
}

#[test]
fn local_leased_knot_ids_includes_the_lease_knot_and_its_bound_knot() {
    let (_db_ws, path) = unique_db_path();
    let conn = open_connection(&path).expect("connection should open");

    insert_active_lease(&conn, "K-lease", &owned_by("machine-x"));
    insert_bound_work_knot(&conn, "K-work", "K-lease");

    let leased = local_leased_knot_ids(&conn, "machine-x").expect("query should succeed");
    assert!(
        leased.contains("K-lease"),
        "the lease knot itself must be held back"
    );
    assert!(
        leased.contains("K-work"),
        "the knot bound to the lease must be held back"
    );
    assert_eq!(leased.len(), 2);
}

#[test]
fn local_leased_knot_ids_excludes_leases_owned_by_another_machine() {
    let (_db_ws, path) = unique_db_path();
    let conn = open_connection(&path).expect("connection should open");

    insert_active_lease(&conn, "K-lease-remote", &owned_by("machine-a"));
    insert_bound_work_knot(&conn, "K-work-remote", "K-lease-remote");

    let leased = local_leased_knot_ids(&conn, "machine-b").expect("query should succeed");
    assert!(
        leased.is_empty(),
        "a remote-held lease must never cause a local skip, got {:?}",
        leased
    );
}
