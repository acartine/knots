//! Replicated leases must not block the machine that merely pulled them.

use std::path::{Path, PathBuf};

use crate::db;
use crate::lease_expiry::compute_expiry_ts;
use crate::remote_init::init_remote_knots_branch;

use super::super::ReplicationService;
use super::{run_git, setup_origin_and_dev1, unique_workspace};

const LEASE_ID: &str = "K-lease-remote";

/// Author a lease knot as a pair of real events so the owner has to survive
/// the event write -> push -> pull -> knot_hot path.
fn write_lease_events(store_root: &Path, machine_id: &str) {
    let index_dir = store_root.join("index/2026/03/12");
    std::fs::create_dir_all(&index_dir).expect("index dir should be creatable");
    std::fs::write(
        index_dir.join("9101-idx.knot_head.json"),
        format!(
            concat!(
                "{{\n",
                "  \"event_id\": \"9101\",\n",
                "  \"occurred_at\": \"2026-03-12T10:00:00Z\",\n",
                "  \"type\": \"idx.knot_head\",\n",
                "  \"data\": {{\n",
                "    \"knot_id\": \"{id}\",\n",
                "    \"title\": \"Lease: remote agent\",\n",
                "    \"state\": \"lease_active\",\n",
                "    \"type\": \"lease\",\n",
                "    \"workflow_id\": \"lease_sdlc\",\n",
                "    \"profile_id\": \"autopilot\",\n",
                "    \"updated_at\": \"2026-03-12T10:00:00Z\",\n",
                "    \"terminal\": false\n",
                "  }}\n",
                "}}\n"
            ),
            id = LEASE_ID
        ),
    )
    .expect("index event should be writable");

    let events_dir = store_root.join("events/2026/03/12");
    std::fs::create_dir_all(&events_dir).expect("events dir should be creatable");
    std::fs::write(
        events_dir.join("9102-knot.lease_data_set.json"),
        format!(
            concat!(
                "{{\n",
                "  \"event_id\": \"9102\",\n",
                "  \"occurred_at\": \"2026-03-12T10:00:01Z\",\n",
                "  \"knot_id\": \"{id}\",\n",
                "  \"type\": \"knot.lease_data_set\",\n",
                "  \"data\": {{\"lease_data\": {{\n",
                "    \"lease_type\": \"agent\",\n",
                "    \"nickname\": \"remote-agent\",\n",
                "    \"timeout_seconds\": 600,\n",
                "    \"owner\": {{\"machine_id\": \"{machine}\", \"pid\": 4242}}\n",
                "  }}}}\n",
                "}}\n"
            ),
            id = LEASE_ID,
            machine = machine_id
        ),
    )
    .expect("lease data event should be writable");
}

/// Publish a lease owned by `machine_id` from dev1 and pull it into a fresh
/// dev2 clone. Returns the workspace root and dev2's path.
fn publish_lease_and_clone(machine_id: &str) -> (PathBuf, PathBuf) {
    let root = unique_workspace();
    let (origin, dev1) = setup_origin_and_dev1(&root);
    write_lease_events(&dev1.join(".knots"), machine_id);
    init_remote_knots_branch(&dev1).expect("remote knots branch should initialize");

    let conn1 = open_store_db(&dev1);
    let service1 = ReplicationService::new(&conn1, dev1.clone());
    assert!(service1.push().expect("push should succeed").pushed);

    let dev2 = root.join("dev2");
    run_git(
        &root,
        &[
            "clone",
            origin.to_str().expect("utf8 path"),
            dev2.to_str().expect("utf8 path"),
        ],
    );
    run_git(&dev2, &["config", "user.email", "knots@example.com"]);
    run_git(&dev2, &["config", "user.name", "Knots Test"]);
    (root, dev2)
}

fn open_store_db(repo_root: &Path) -> rusqlite::Connection {
    let path = repo_root.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(path.parent().expect("db parent should exist"))
        .expect("db parent should be creatable");
    db::open_connection(path.to_str().expect("utf8 path")).expect("db should open")
}

#[test]
fn pulled_foreign_lease_survives_sync_and_does_not_block_it() {
    let (root, dev2) = publish_lease_and_clone("machine-a");

    let conn2 = open_store_db(&dev2);
    db::set_meta(&conn2, "hot_window_days", "365").expect("hot_window_days should be settable");
    let service2 = ReplicationService::new(&conn2, dev2.clone());
    service2.pull().expect("pull should succeed");

    // The owner round-tripped through event write -> push -> pull -> knot_hot.
    let record = db::get_knot_hot(&conn2, LEASE_ID)
        .expect("knot query should succeed")
        .expect("lease should be present after pull");
    let owner = record
        .lease_data
        .owner
        .as_ref()
        .expect("owner should survive replication");
    assert_eq!(owner.machine_id, "machine-a");
    assert_eq!(owner.pid, 4242);

    // `write_lease_events` above doesn't set `lease_expiry_ts` on its
    // fixture, so the pulled row keeps the "unknown" default of 0 (see
    // knot e045: absent means unknown, not 0-as-expired -- but 0 still
    // reads as expired via `effective_lease_state`). Set a live expiry
    // directly so this test isolates ownership, not expiry, as the reason
    // sync stays unblocked. Real replication of a live expiry is covered by
    // `tests_lease_expiry_replication.rs`.
    db::update_lease_expiry_ts(&conn2, LEASE_ID, compute_expiry_ts(600))
        .expect("expiry update should succeed");

    let local_machine = crate::machine::machine_id(&dev2.join(".knots"));
    assert_ne!(local_machine, "machine-a");
    assert_eq!(
        db::count_active_leases(&conn2).expect("count should succeed"),
        1,
        "the replicated lease is active by the owner-blind query"
    );
    assert!(
        db::local_leased_knot_ids(&conn2, &local_machine)
            .expect("query should succeed")
            .is_empty(),
        "no agent on this machine holds the replicated lease"
    );

    service2.push().expect("push should not be blocked");
    service2.pull().expect("pull should not be blocked");
    service2.sync().expect("sync should not be blocked");
    let mut reporter = None;
    let outcome = service2
        .sync_or_defer_with_progress(&mut reporter)
        .expect("sync_or_defer should succeed");
    assert!(
        matches!(outcome, super::super::SyncOutcome::Completed(_)),
        "expected Completed, got {:?}",
        outcome
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn lease_owned_by_this_machine_no_longer_blocks_the_pull_half() {
    let (root, dev2) = publish_lease_and_clone("placeholder-machine");

    let conn2 = open_store_db(&dev2);
    db::set_meta(&conn2, "hot_window_days", "365").expect("hot_window_days should be settable");
    let service2 = ReplicationService::new(&conn2, dev2.clone());
    service2.pull().expect("pull should succeed");

    // Re-stamp the pulled lease with dev2's own machine id: this is the
    // "an agent here holds it" case, which must now be filtered per knot
    // instead of blocking the whole pull.
    let local_machine = crate::machine::machine_id(&dev2.join(".knots"));
    let owned = format!(
        r#"{{"lease_type":"agent","nickname":"local","owner":{{"machine_id":"{}","pid":7}}}}"#,
        local_machine
    );
    conn2
        .execute(
            "UPDATE knot_hot SET lease_data_json = ?1 WHERE id = ?2",
            rusqlite::params![owned, LEASE_ID],
        )
        .expect("lease data update should succeed");
    db::update_lease_expiry_ts(&conn2, LEASE_ID, compute_expiry_ts(600))
        .expect("expiry update should succeed");

    assert!(
        db::local_leased_knot_ids(&conn2, &local_machine)
            .expect("query should succeed")
            .contains(LEASE_ID),
        "the lease knot itself must be in the local-leased set"
    );

    service2
        .push()
        .expect("push is never blocked, even by a locally held lease");

    service2
        .pull()
        .expect("pull no longer refuses with a locally held lease");

    let mut reporter = None;
    let outcome = service2
        .sync_or_defer_with_progress(&mut reporter)
        .expect("sync_or_defer should succeed");
    assert!(
        matches!(outcome, super::super::SyncOutcome::Completed(_)),
        "expected Completed, got {:?}",
        outcome
    );

    let _ = std::fs::remove_dir_all(root);
}
