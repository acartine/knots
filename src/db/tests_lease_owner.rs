//! Owner-aware active-lease queries.

use super::{cleanup_db_files, unique_db_path};
use crate::db::{
    count_active_leases, count_local_active_leases, open_connection, update_lease_expiry_ts,
    upsert_knot_hot, UpsertKnotHot,
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

#[test]
fn local_active_lease_count_excludes_other_machines() {
    let path = unique_db_path();
    let conn = open_connection(&path).expect("connection should open");

    insert_active_lease(&conn, "K-lease-x", &owned_by("machine-x"));

    assert_eq!(
        count_active_leases(&conn).expect("count should succeed"),
        1,
        "the owner-blind query still sees the lease"
    );
    assert_eq!(
        count_local_active_leases(&conn, "machine-x").expect("count should succeed"),
        1,
        "the owning machine counts its own lease"
    );
    assert_eq!(
        count_local_active_leases(&conn, "machine-y").expect("count should succeed"),
        0,
        "another machine must not count a replicated lease"
    );

    cleanup_db_files(&path);
}

#[test]
fn lease_without_an_owner_counts_as_local() {
    let path = unique_db_path();
    let conn = open_connection(&path).expect("connection should open");

    insert_active_lease(&conn, "K-lease-legacy", &LeaseData::default());

    for machine in ["machine-x", "machine-y"] {
        assert_eq!(
            count_local_active_leases(&conn, machine).expect("count should succeed"),
            1,
            "a legacy owner-less lease stays local for {machine}"
        );
    }

    cleanup_db_files(&path);
}

#[test]
fn each_machine_counts_only_its_own_leases_plus_legacy() {
    let path = unique_db_path();
    let conn = open_connection(&path).expect("connection should open");

    insert_active_lease(&conn, "K-lease-a", &owned_by("machine-x"));
    insert_active_lease(&conn, "K-lease-b", &owned_by("machine-y"));
    insert_active_lease(&conn, "K-lease-c", &LeaseData::default());

    assert_eq!(count_active_leases(&conn).expect("count should succeed"), 3);
    assert_eq!(
        count_local_active_leases(&conn, "machine-x").expect("count should succeed"),
        2,
        "machine-x sees its own lease and the legacy one"
    );
    assert_eq!(
        count_local_active_leases(&conn, "machine-y").expect("count should succeed"),
        2,
        "machine-y sees its own lease and the legacy one"
    );

    cleanup_db_files(&path);
}

#[test]
fn expired_and_terminated_leases_are_never_local_active() {
    let path = unique_db_path();
    let conn = open_connection(&path).expect("connection should open");

    insert_active_lease(&conn, "K-lease-expired", &owned_by("machine-x"));
    update_lease_expiry_ts(&conn, "K-lease-expired", 1).expect("expiry should be settable");

    assert_eq!(
        count_local_active_leases(&conn, "machine-x").expect("count should succeed"),
        0
    );

    cleanup_db_files(&path);
}
