use super::{cleanup_db_files, unique_db_path};
use crate::db::{
    list_knot_hot, list_knot_hot_paginated, open_connection, upsert_knot_hot, ListHotParams,
    UpsertKnotHot,
};
use crate::domain::gate::GateData;
use crate::domain::lease::LeaseData;

fn insert_test_knots(conn: &rusqlite::Connection, count: usize) {
    let gate_data = GateData::default();
    let lease_data = LeaseData::default();
    for i in 0..count {
        let id = format!("K-{:03}", i);
        let title = format!("Knot {}", i);
        let state = if i % 3 == 0 {
            "planning"
        } else if i % 3 == 1 {
            "implementation"
        } else {
            "shipped"
        };
        let knot_type = if i % 5 == 0 {
            Some("gate")
        } else {
            Some("work")
        };
        let updated = format!("2026-04-06T{:02}:00:00Z", i % 24);
        upsert_knot_hot(
            conn,
            &UpsertKnotHot {
                id: &id,
                title: &title,
                state,
                updated_at: &updated,
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
                lease_data: &lease_data,
                lease_id: None,
                workflow_id: "work_sdlc",
                profile_id: "autopilot",
                profile_etag: None,
                deferred_from_state: None,
                blocked_from_state: None,
                created_at: None,
            },
        )
        .expect("upsert should succeed");
    }
}

#[test]
fn paginated_returns_full_set_without_limit() {
    let path = unique_db_path();
    let conn = open_connection(&path).expect("open");
    insert_test_knots(&conn, 10);

    let params = ListHotParams::default();
    let (records, total) = list_knot_hot_paginated(&conn, &params).expect("query");
    assert_eq!(total, 10);
    assert_eq!(records.len(), 10);

    cleanup_db_files(&path);
}

#[test]
fn paginated_limit_returns_subset() {
    let path = unique_db_path();
    let conn = open_connection(&path).expect("open");
    insert_test_knots(&conn, 10);

    let params = ListHotParams {
        limit: Some(3),
        ..Default::default()
    };
    let (records, total) = list_knot_hot_paginated(&conn, &params).expect("query");
    assert_eq!(total, 10);
    assert_eq!(records.len(), 3);

    cleanup_db_files(&path);
}

#[test]
fn paginated_offset_skips_rows() {
    let path = unique_db_path();
    let conn = open_connection(&path).expect("open");
    insert_test_knots(&conn, 10);

    let all = list_knot_hot(&conn).expect("full list");
    let params = ListHotParams {
        limit: Some(3),
        offset: Some(2),
        ..Default::default()
    };
    let (records, total) = list_knot_hot_paginated(&conn, &params).expect("query");
    assert_eq!(total, 10);
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].id, all[2].id);
    assert_eq!(records[1].id, all[3].id);
    assert_eq!(records[2].id, all[4].id);

    cleanup_db_files(&path);
}

#[test]
fn paginated_state_filter_narrows_results() {
    let path = unique_db_path();
    let conn = open_connection(&path).expect("open");
    insert_test_knots(&conn, 12);

    let params = ListHotParams {
        state: Some("planning".to_string()),
        ..Default::default()
    };
    let (records, total) = list_knot_hot_paginated(&conn, &params).expect("query");
    assert_eq!(total as usize, records.len());
    for r in &records {
        assert_eq!(r.state, "planning");
    }

    cleanup_db_files(&path);
}

#[test]
fn paginated_knot_type_filter_narrows_results() {
    let path = unique_db_path();
    let conn = open_connection(&path).expect("open");
    insert_test_knots(&conn, 10);

    let params = ListHotParams {
        knot_type: Some("gate".to_string()),
        ..Default::default()
    };
    let (records, total) = list_knot_hot_paginated(&conn, &params).expect("query");
    assert_eq!(total as usize, records.len());
    for r in &records {
        assert_eq!(r.knot_type.as_deref(), Some("gate"));
    }

    cleanup_db_files(&path);
}

#[test]
fn paginated_state_filter_with_limit() {
    let path = unique_db_path();
    let conn = open_connection(&path).expect("open");
    insert_test_knots(&conn, 30);

    let all_planning = ListHotParams {
        state: Some("planning".to_string()),
        ..Default::default()
    };
    let (_, total_planning) = list_knot_hot_paginated(&conn, &all_planning).expect("count");

    let params = ListHotParams {
        state: Some("planning".to_string()),
        limit: Some(2),
        offset: Some(0),
        ..Default::default()
    };
    let (records, total) = list_knot_hot_paginated(&conn, &params).expect("query");
    assert_eq!(total, total_planning);
    assert_eq!(records.len(), 2);
    for r in &records {
        assert_eq!(r.state, "planning");
    }

    cleanup_db_files(&path);
}

#[test]
fn paginated_offset_beyond_results_returns_empty() {
    let path = unique_db_path();
    let conn = open_connection(&path).expect("open");
    insert_test_knots(&conn, 5);

    let params = ListHotParams {
        limit: Some(10),
        offset: Some(100),
        ..Default::default()
    };
    let (records, total) = list_knot_hot_paginated(&conn, &params).expect("query");
    assert_eq!(total, 5);
    assert!(records.is_empty());

    cleanup_db_files(&path);
}

#[test]
fn paginated_preserves_order() {
    let path = unique_db_path();
    let conn = open_connection(&path).expect("open");
    insert_test_knots(&conn, 10);

    let full = list_knot_hot(&conn).expect("full");
    let params = ListHotParams {
        limit: Some(10),
        ..Default::default()
    };
    let (paginated, _) = list_knot_hot_paginated(&conn, &params).expect("query");
    let full_ids: Vec<&str> = full.iter().map(|r| r.id.as_str()).collect();
    let pag_ids: Vec<&str> = paginated.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(full_ids, pag_ids);

    cleanup_db_files(&path);
}
