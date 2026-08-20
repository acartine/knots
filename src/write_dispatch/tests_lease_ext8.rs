//! Root-cause regression for knot e045: `App::set_lease_expiry` -- the
//! single choke point every expiry change funnels through (creation,
//! heartbeat, explicit extend, lazy-recovery re-arm) -- must push a fresh
//! `idx.knot_head` event carrying the new expiry, so a `kno push` actually
//! replicates it. Before this fix it only updated the local DB column and
//! never wrote any event at all.

use crate::poll_claim;

use super::tests_lease_ext::{open_app, setup_repo, unique_workspace};

fn claim_work_knot(app: &crate::app::App, timeout: u64) -> (String, String) {
    let work = app
        .create_knot(
            "Lease expiry replication test",
            None,
            Some("work_item"),
            Some("default"),
        )
        .expect("create work knot");
    let claimed = poll_claim::claim_knot(
        app,
        &work.id,
        Some("agent".to_string()),
        None,
        timeout,
        false,
    )
    .expect("claim should succeed");
    let lease_id = claimed.knot.lease_id.clone().expect("should have lease");
    (work.id, lease_id)
}

fn count_json_files(root: &std::path::Path) -> usize {
    if !root.exists() {
        return 0;
    }
    let mut count = 0usize;
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(dir).expect("directory should be readable") {
            let path = entry.expect("entry should be readable").path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|ext| ext == "json") {
                count += 1;
            }
        }
    }
    count
}

/// `set_lease_expiry` must write exactly one new `idx.knot_head` event per
/// call, and that event's `updated_at` payload field must stay pinned to
/// the record's existing `updated_at`: a heartbeat is not a content change
/// and must not reorder `kno ls`.
#[test]
fn set_lease_expiry_writes_a_replicated_head_event_without_bumping_updated_at() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let app = open_app(&root);

    let (_knot_id, lease_id) = claim_work_knot(&app, 600);
    let lease_before = app
        .show_knot(&lease_id)
        .expect("show")
        .expect("lease exists");
    let index_dir = root.join(".knots/index");
    let events_before = count_json_files(&index_dir);

    let new_ts = crate::lease_expiry::compute_expiry_ts(9_999);
    app.set_lease_expiry(&lease_id, new_ts)
        .expect("set expiry should succeed");

    let events_after = count_json_files(&index_dir);
    assert_eq!(
        events_after,
        events_before + 1,
        "set_lease_expiry must write exactly one new idx.knot_head event"
    );

    // The DB row is updated locally, same as before this fix.
    let hot = crate::db::get_knot_hot(app.conn_for_test(), &lease_id)
        .expect("hot lookup should succeed")
        .expect("lease should be cached");
    assert_eq!(hot.lease_expiry_ts, new_ts);
    assert_eq!(
        hot.updated_at, lease_before.updated_at,
        "a heartbeat must not bump the knot's visible updated_at"
    );

    // The event actually on disk carries the new expiry, which is what a
    // pull on another machine would apply.
    let mut found = false;
    let mut dirs = vec![index_dir.clone()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).expect("dir readable") {
            let path = entry.expect("entry readable").path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let body = std::fs::read_to_string(&path).expect("event readable");
            let json: serde_json::Value = serde_json::from_str(&body).expect("event is JSON");
            if json["type"] == "idx.knot_head"
                && json["data"]["knot_id"] == lease_id
                && json["data"]["lease_expiry_ts"] == new_ts
            {
                found = true;
            }
        }
    }
    assert!(
        found,
        "expected an idx.knot_head event on disk for the lease with the new expiry"
    );
}
