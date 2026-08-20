//! Push is never blocked by a lease, so a deferred sync still publishes.

use std::path::Path;
use std::process::Command;

use crate::db;
use crate::lease_expiry::compute_expiry_ts;
use crate::remote_init::init_remote_knots_branch;
use crate::sync_ref::SyncRefConfig;

use super::super::{ReplicationService, SyncOutcome};
use super::{setup_origin_and_dev1, unique_workspace};

const LEASE_ID: &str = "K-lease-busy";
const EVENT_REL: &str = "events/2026/03/12/9201-knot.note_added.json";
const NOTE_TEXT: &str = "published while a lease was held";

/// Author a real event file so the push half has something to publish.
fn write_note_event(store_root: &Path) {
    let path = store_root.join(EVENT_REL);
    std::fs::create_dir_all(path.parent().expect("event parent should exist"))
        .expect("events dir should be creatable");
    std::fs::write(
        &path,
        format!(
            concat!(
                "{{\n",
                "  \"event_id\": \"9201\",\n",
                "  \"occurred_at\": \"2026-03-12T10:00:00Z\",\n",
                "  \"knot_id\": \"K-busy\",\n",
                "  \"type\": \"knot.note_added\",\n",
                "  \"data\": {{\"text\": \"{text}\"}}\n",
                "}}\n"
            ),
            text = NOTE_TEXT
        ),
    )
    .expect("note event should be writable");
}

/// Record an owner-less active lease, which `count_local_active_leases`
/// treats as held by this machine.
fn hold_active_local_lease(conn: &rusqlite::Connection) {
    let gate_data = crate::domain::gate::GateData::default();
    db::upsert_knot_hot(
        conn,
        &db::UpsertKnotHot {
            id: LEASE_ID,
            title: "Lease: busy agent",
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
            gate_data: &gate_data,
            lease_data: &crate::domain::lease::LeaseData::default(),
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
    db::update_lease_expiry_ts(conn, LEASE_ID, compute_expiry_ts(600))
        .expect("expiry update should succeed");
}

/// Read a blob from the bare origin at the configured knots ref.
fn cat_file_on_knots_ref(origin: &Path, config: &SyncRefConfig, rel: &str) -> Option<String> {
    let spec = format!("{}:.knots/{rel}", config.remote_ref());
    let output = Command::new("git")
        .arg("--git-dir")
        .arg(origin)
        .args(["cat-file", "-p", &spec])
        .output()
        .expect("git cat-file should run");
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[test]
fn deferred_sync_publishes_its_events_to_the_knots_ref() {
    let root = unique_workspace();
    let (origin, dev1) = setup_origin_and_dev1(&root);
    write_note_event(&dev1.join(".knots"));
    init_remote_knots_branch(&dev1).expect("remote knots branch should initialize");

    let db_path = dev1.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(db_path.parent().expect("db parent should exist"))
        .expect("db parent should be creatable");
    let conn = db::open_connection(db_path.to_str().expect("utf8 path")).expect("db should open");
    hold_active_local_lease(&conn);

    let config = SyncRefConfig::for_repo(&dev1);
    assert!(
        cat_file_on_knots_ref(&origin, &config, EVENT_REL).is_none(),
        "the event must not be on the remote before the sync"
    );

    let service = ReplicationService::new(&conn, dev1.clone());
    let mut reporter = None;
    let outcome = service
        .sync_or_defer_with_progress(&mut reporter)
        .expect("sync_or_defer should succeed while a lease is held");

    let SyncOutcome::Deferred {
        active_leases,
        push,
    } = outcome
    else {
        panic!("expected the pull half to defer, got {:?}", outcome);
    };
    assert_eq!(active_leases, 1);
    assert_eq!(push.local_event_files, 1, "the note event was scanned");
    assert_eq!(push.copied_files, 1, "the note event was staged");
    assert!(push.committed, "the push half committed");
    assert!(push.pushed, "the push half reached the remote");
    assert!(
        push.commit.is_some(),
        "the deferred summary names the commit"
    );

    // The summary is only a claim; prove the bytes landed on the ref.
    let published = cat_file_on_knots_ref(&origin, &config, EVENT_REL)
        .expect("the event should be readable on the knots ref");
    assert!(
        published.contains(NOTE_TEXT),
        "published blob should carry the event body, got {published}"
    );

    let _ = std::fs::remove_dir_all(root);
}
