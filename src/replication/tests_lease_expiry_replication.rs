//! End-to-end push/pull round trip for `lease_expiry_ts` replication
//! (knot e045, verification step 1): a lease pushed from machine A with a
//! future expiry must be readable on machine B with that same expiry after
//! pull, and must not read as expired there.

use std::path::Path;

use crate::db;
use crate::lease_expiry::effective_lease_state;

use super::super::ReplicationService;
use super::{run_git, setup_origin_and_dev1, unique_workspace};

const LEASE_ID: &str = "K-lease-expiry-roundtrip";

fn write_lease_head_event(store_root: &Path, lease_expiry_ts: i64) {
    let index_dir = store_root.join("index/2026/03/12");
    std::fs::create_dir_all(&index_dir).expect("index dir should be creatable");
    std::fs::write(
        index_dir.join("9201-idx.knot_head.json"),
        format!(
            concat!(
                "{{\n",
                "  \"event_id\": \"9201\",\n",
                "  \"occurred_at\": \"2026-03-12T10:00:00Z\",\n",
                "  \"type\": \"idx.knot_head\",\n",
                "  \"data\": {{\n",
                "    \"knot_id\": \"{id}\",\n",
                "    \"title\": \"Lease: roundtrip agent\",\n",
                "    \"state\": \"lease_active\",\n",
                "    \"type\": \"lease\",\n",
                "    \"workflow_id\": \"lease_sdlc\",\n",
                "    \"profile_id\": \"autopilot\",\n",
                "    \"updated_at\": \"2026-03-12T10:00:00Z\",\n",
                "    \"terminal\": false,\n",
                "    \"lease_expiry_ts\": {expiry}\n",
                "  }}\n",
                "}}\n"
            ),
            id = LEASE_ID,
            expiry = lease_expiry_ts,
        ),
    )
    .expect("index event should be writable");
}

fn open_store_db(repo_root: &Path) -> rusqlite::Connection {
    let path = repo_root.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(path.parent().expect("db parent should exist"))
        .expect("db parent should be creatable");
    db::open_connection(path.to_str().expect("utf8 path")).expect("db should open")
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_secs() as i64
}

#[test]
fn lease_expiry_pushed_from_one_machine_reads_correctly_after_pull_on_another() {
    let root = unique_workspace();
    let (origin, dev1) = setup_origin_and_dev1(&root);
    let future_expiry = now_unix() + 3_600;
    write_lease_head_event(&dev1.join(".knots"), future_expiry);
    crate::remote_init::init_remote_knots_branch(&dev1)
        .expect("remote knots branch should initialize");

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

    let conn2 = open_store_db(&dev2);
    db::set_meta(&conn2, "hot_window_days", "365").expect("hot_window_days should be settable");
    let service2 = ReplicationService::new(&conn2, dev2.clone());
    service2.pull().expect("pull should succeed");

    let record = db::get_knot_hot(&conn2, LEASE_ID)
        .expect("knot query should succeed")
        .expect("lease should be present after pull");
    assert_eq!(
        record.lease_expiry_ts, future_expiry,
        "the expiry pushed from dev1 must read back identically on dev2"
    );
    assert_eq!(
        effective_lease_state(&record.state, record.lease_expiry_ts),
        "lease_active",
        "a lease with real remaining time must not read as expired on the pulling machine"
    );

    let _ = std::fs::remove_dir_all(root);
}
