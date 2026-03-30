use super::{cleanup_db_files, unique_db_path};
use crate::db::{
    get_sync_fetch_blob_limit_kb, needs_schema_bootstrap, open_connection, set_meta,
    CURRENT_SCHEMA_VERSION,
};
use rusqlite::params;

#[test]
fn upsert_and_get_knot_hot_round_trips_invariants() {
    use crate::db::{get_knot_hot, upsert_knot_hot, UpsertKnotHot};
    use crate::domain::invariant::{Invariant, InvariantType};

    let path = unique_db_path();
    let conn = open_connection(&path).expect("connection should open");

    let invariants = vec![
        Invariant::new(InvariantType::Scope, "only touch src/db.rs").unwrap(),
        Invariant::new(InvariantType::State, "coverage >= 95%").unwrap(),
    ];

    upsert_knot_hot(
        &conn,
        &UpsertKnotHot {
            id: "K-inv",
            title: "Invariant round-trip",
            state: "implementation",
            updated_at: "2026-03-05T10:00:00Z",
            body: None,
            description: Some("test invariants"),
            acceptance: None,
            priority: None,
            knot_type: Some("work"),
            tags: &["alpha".to_string()],
            notes: &[],
            handoff_capsules: &[],
            invariants: &invariants,
            step_history: &[],
            gate_data: &crate::domain::gate::GateData::default(),
            lease_data: &crate::domain::lease::LeaseData::default(),
            lease_id: None,
            workflow_id: "compatibility",
            profile_id: "autopilot",
            profile_etag: Some("etag-inv"),
            deferred_from_state: None,
            blocked_from_state: None,
            created_at: Some("2026-03-05T09:00:00Z"),
        },
    )
    .expect("upsert with invariants should succeed");

    let record = get_knot_hot(&conn, "K-inv")
        .expect("get should succeed")
        .expect("record should exist");
    assert_eq!(record.invariants.len(), 2);
    assert_eq!(record.invariants[0].invariant_type, InvariantType::Scope);
    assert_eq!(record.invariants[0].condition, "only touch src/db.rs");
    assert_eq!(record.invariants[1].invariant_type, InvariantType::State);
    assert_eq!(record.invariants[1].condition, "coverage >= 95%");

    cleanup_db_files(&path);
}

#[test]
fn upsert_knot_hot_with_empty_invariants_round_trips() {
    use crate::db::{get_knot_hot, upsert_knot_hot, UpsertKnotHot};

    let path = unique_db_path();
    let conn = open_connection(&path).expect("connection should open");

    upsert_knot_hot(
        &conn,
        &UpsertKnotHot {
            id: "K-no-inv",
            title: "No invariants",
            state: "ready_for_planning",
            updated_at: "2026-03-05T10:00:00Z",
            body: None,
            description: None,
            acceptance: None,
            priority: None,
            knot_type: None,
            tags: &[],
            notes: &[],
            handoff_capsules: &[],
            invariants: &[],
            step_history: &[],
            gate_data: &crate::domain::gate::GateData::default(),
            lease_data: &crate::domain::lease::LeaseData::default(),
            lease_id: None,
            workflow_id: "compatibility",
            profile_id: "autopilot",
            profile_etag: None,
            deferred_from_state: None,
            blocked_from_state: None,
            created_at: None,
        },
    )
    .expect("upsert with empty invariants should succeed");

    let record = get_knot_hot(&conn, "K-no-inv")
        .expect("get should succeed")
        .expect("record should exist");
    assert!(record.invariants.is_empty());

    cleanup_db_files(&path);
}

#[test]
fn count_active_leases_returns_count() {
    use crate::db::{count_active_leases, upsert_knot_hot, UpsertKnotHot};
    use crate::domain::lease::LeaseData;

    let path = unique_db_path();
    let conn = open_connection(&path).expect("connection should open");

    let empty = count_active_leases(&conn).expect("count should succeed on empty db");
    assert_eq!(empty, 0);

    let gate_data = crate::domain::gate::GateData::default();
    for (id, state) in [
        ("K-lease-1", "lease_ready"),
        ("K-lease-2", "lease_active"),
        ("K-lease-3", "lease_terminated"),
        ("K-work-1", "implementation"),
    ] {
        let knot_type = if id.starts_with("K-lease") {
            Some("lease")
        } else {
            Some("work")
        };
        upsert_knot_hot(
            &conn,
            &UpsertKnotHot {
                id,
                title: id,
                state,
                updated_at: "2026-03-12T00:00:00Z",
                body: None,
                description: None,
                acceptance: None,
                priority: None,
                knot_type,
                tags: &[],
                notes: &[],
                handoff_capsules: &[],
                invariants: &[],
                step_history: &[],
                gate_data: &gate_data,
                lease_data: &LeaseData::default(),
                lease_id: None,
                workflow_id: "compatibility",
                profile_id: "autopilot",
                profile_etag: None,
                deferred_from_state: None,
                blocked_from_state: None,
                created_at: None,
            },
        )
        .expect("upsert should succeed");
    }

    let count = count_active_leases(&conn).expect("count should succeed");
    assert_eq!(count, 2);

    cleanup_db_files(&path);
}

#[test]
fn get_knot_hot_accepts_legacy_empty_lease_data_json() {
    use crate::db::{get_knot_hot, upsert_knot_hot, UpsertKnotHot};

    let path = unique_db_path();
    let conn = open_connection(&path).expect("connection should open");

    upsert_knot_hot(
        &conn,
        &UpsertKnotHot {
            id: "K-legacy-lease",
            title: "Legacy lease",
            state: "implementation",
            updated_at: "2026-03-18T10:00:00Z",
            body: None,
            description: None,
            acceptance: None,
            priority: None,
            knot_type: Some("work"),
            tags: &[],
            notes: &[],
            handoff_capsules: &[],
            invariants: &[],
            step_history: &[],
            gate_data: &crate::domain::gate::GateData::default(),
            lease_data: &crate::domain::lease::LeaseData::default(),
            lease_id: None,
            workflow_id: "compatibility",
            profile_id: "autopilot",
            profile_etag: None,
            deferred_from_state: None,
            blocked_from_state: None,
            created_at: None,
        },
    )
    .expect("upsert should succeed");

    conn.execute(
        "UPDATE knot_hot SET lease_data_json = '{}' WHERE id = ?1",
        params!["K-legacy-lease"],
    )
    .expect("legacy lease payload should update");

    let record = get_knot_hot(&conn, "K-legacy-lease")
        .expect("legacy read should succeed")
        .expect("record should exist");
    assert_eq!(
        record.lease_data,
        crate::domain::lease::LeaseData::default()
    );

    cleanup_db_files(&path);
}

#[test]
fn needs_schema_bootstrap_detects_meta_drift() {
    let path = unique_db_path();
    let conn = open_connection(&path).expect("connection should open");
    assert!(!needs_schema_bootstrap(&conn).expect("fresh schema should not need bootstrap"));

    set_meta(&conn, "schema_version", "0").expect("schema version should update");
    assert!(needs_schema_bootstrap(&conn).expect("stale schema version should trigger bootstrap"));

    set_meta(&conn, "schema_version", &CURRENT_SCHEMA_VERSION.to_string())
        .expect("schema version should restore");
    conn.execute("DELETE FROM meta WHERE key = 'sync_policy'", [])
        .expect("required meta key should delete");
    assert!(needs_schema_bootstrap(&conn).expect("missing meta should trigger bootstrap"));

    cleanup_db_files(&path);
}

#[test]
fn fetch_blob_limit_env_override_covers_env_path() {
    let path = unique_db_path();
    let conn = open_connection(&path).expect("connection should open");
    set_meta(&conn, "sync_fetch_blob_limit_kb", "4").expect("meta update should succeed");

    std::env::set_var("KNOTS_FETCH_BLOB_LIMIT_KB", "8");
    let env_value = get_sync_fetch_blob_limit_kb(&conn).expect("env override should parse");
    std::env::remove_var("KNOTS_FETCH_BLOB_LIMIT_KB");
    assert_eq!(env_value, Some(8));

    cleanup_db_files(&path);
}
