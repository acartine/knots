use std::cell::{Cell, RefCell};

use rusqlite::Connection;
use sha2::{Digest, Sha256};

use super::*;
use crate::compaction::{
    validate_protection, ProtectionMarker, ProviderProtectionFacts, V2RefLayout,
};
use crate::db::{
    inventory_legacy_files, list_pending_outbox, record_outbox_event, rotate_writer_epoch,
};

#[derive(Default)]
struct FakeTransport {
    remote_oid: RefCell<Option<String>>,
    publish_calls: Cell<usize>,
    fail_after_remote_update: Cell<bool>,
}

impl InboxTransport for FakeTransport {
    fn prepare(
        &self,
        writer: &WriterEpoch,
        events: &[OutboxRecord],
        expected_control_oid: Option<&str>,
    ) -> Result<PreparedInbox, SyncError> {
        let mut digest = Sha256::new();
        digest.update(writer.writer_id.as_bytes());
        digest.update(writer.expected_inbox_oid.as_deref().unwrap_or_default());
        for event in events {
            digest.update(event.event_id.as_bytes());
            digest.update(event.content_sha256.as_bytes());
        }
        Ok(PreparedInbox {
            writer_id: writer.writer_id.clone(),
            inbox_ref: writer.inbox_ref.clone(),
            proposed_oid: format!("{:x}", digest.finalize()),
            expected_inbox_oid: writer.expected_inbox_oid.clone(),
            expected_control_oid: expected_control_oid.map(str::to_string),
            includes_control_registration: !writer.registered,
            event_ids: events.iter().map(|event| event.event_id.clone()).collect(),
        })
    }

    fn publish(&self, prepared: &PreparedInbox) -> Result<(), SyncError> {
        self.publish_calls.set(self.publish_calls.get() + 1);
        *self.remote_oid.borrow_mut() = Some(prepared.proposed_oid.clone());
        if self.fail_after_remote_update.replace(false) {
            return Err(outbox_error("simulated lost acknowledgement"));
        }
        Ok(())
    }

    fn fetch_inbox_oid(&self, _inbox_ref: &str) -> Result<Option<String>, SyncError> {
        Ok(self.remote_oid.borrow().clone())
    }
}

#[test]
fn publisher_reads_only_receipts_and_confirms_exact_remote_oid() {
    let conn = test_connection();
    record(&conn, "local-a", b"a");
    let transport = FakeTransport::default();
    let result = OutboxPublisher::new(&conn, &transport)
        .publish_pending("credential-a", &protection(Some("control-a")), 10)
        .expect("publish should succeed")
        .expect("one batch should publish");

    assert_eq!(result.acknowledged_events, 1);
    assert!(result.registered_writer);
    assert_eq!(transport.publish_calls.get(), 1);
    assert!(list_pending_outbox(&conn, &result.writer_id, 10)
        .expect("query pending")
        .is_empty());
}

#[test]
fn lost_acknowledgement_recovers_without_republishing() {
    let conn = test_connection();
    record(&conn, "local-a", b"a");
    let transport = FakeTransport::default();
    transport.fail_after_remote_update.set(true);
    let publisher = OutboxPublisher::new(&conn, &transport);

    assert!(publisher
        .publish_pending("credential-a", &protection(None), 10)
        .is_err());
    let recovered = publisher
        .publish_pending("credential-a", &protection(None), 10)
        .expect("lost acknowledgement should recover")
        .expect("batch should be acknowledged");

    assert_eq!(recovered.acknowledged_events, 1);
    assert_eq!(transport.publish_calls.get(), 1);
}

#[test]
fn credential_rotation_uses_a_new_writer_scoped_ref() {
    let conn = test_connection();
    let first = ensure_writer_epoch(&conn, "credential-a").expect("first writer");
    let second = rotate_writer_epoch(&conn, "credential-b").expect("rotated writer");

    assert_ne!(first.writer_id, second.writer_id);
    assert_eq!(second.credential_id, "credential-b");
    assert_eq!(
        second.inbox_ref,
        V2RefLayout::default().inbox(&second.writer_id)
    );
}

#[test]
fn exact_event_id_hash_conflicts_fail_closed() {
    let conn = test_connection();
    record(&conn, "same-id", b"first");
    let result = record_outbox_event(
        &conn,
        "same-id",
        "events",
        "events/2026/08/25/same-id.json",
        &digest(b"second"),
        b"second",
    );
    assert!(result.is_err());
}

#[test]
fn legacy_inventory_quarantines_but_never_enqueues_replica_files() {
    let workspace = knots_test_support::workspace("knots-legacy-inventory");
    let root = workspace.path().join(".knots");
    let event_dir = root.join("events/2026/08/25");
    std::fs::create_dir_all(&event_dir).expect("create event directory");
    std::fs::write(event_dir.join("legacy.json"), b"legacy").expect("write legacy event");
    let conn = test_connection();

    assert_eq!(inventory_legacy_files(&conn, &root).expect("inventory"), 1);
    let quarantine: i64 = conn
        .query_row("SELECT COUNT(*) FROM v2_legacy_quarantine", [], |row| {
            row.get(0)
        })
        .expect("count quarantine");
    let outbox: i64 = conn
        .query_row("SELECT COUNT(*) FROM v2_outbox", [], |row| row.get(0))
        .expect("count outbox");
    assert_eq!((quarantine, outbox), (1, 0));
    assert!(event_dir.join("legacy.json").exists());
}

#[test]
fn two_clones_publish_only_their_own_durable_receipts() {
    let clone_a = test_connection();
    let clone_b = test_connection();
    record(&clone_a, "event-a", b"from-a");
    record(&clone_b, "event-b", b"from-b");
    let transport_a = FakeTransport::default();
    let transport_b = FakeTransport::default();

    let result_a = OutboxPublisher::new(&clone_a, &transport_a)
        .publish_pending("credential-a", &protection(None), 10)
        .expect("clone A publish")
        .expect("clone A batch");
    let result_b = OutboxPublisher::new(&clone_b, &transport_b)
        .publish_pending("credential-b", &protection(None), 10)
        .expect("clone B publish")
        .expect("clone B batch");

    assert_ne!(result_a.writer_id, result_b.writer_id);
    let a_events: i64 = clone_a
        .query_row(
            "SELECT COUNT(*) FROM v2_outbox WHERE event_id = 'event-b'",
            [],
            |row| row.get(0),
        )
        .expect("clone A isolation count");
    let b_events: i64 = clone_b
        .query_row(
            "SELECT COUNT(*) FROM v2_outbox WHERE event_id = 'event-a'",
            [],
            |row| row.get(0),
        )
        .expect("clone B isolation count");
    assert_eq!((a_events, b_events), (0, 0));
}

fn test_connection() -> Connection {
    crate::db::open_connection(":memory:").expect("open test database")
}

fn record(conn: &Connection, event_id: &str, payload: &[u8]) {
    record_outbox_event(
        conn,
        event_id,
        "events",
        &format!("events/2026/08/25/{event_id}.json"),
        &digest(payload),
        payload,
    )
    .expect("record outbox event");
}

fn protection(control_head: Option<&str>) -> ValidatedProtection {
    let policy = b"provider-policy";
    let refs = V2RefLayout::default();
    let marker = ProtectionMarker {
        schema_version: 1,
        repository_id: "repo".to_string(),
        policy_id: "policy".to_string(),
        policy_sha256: digest(policy),
        integrator_id: "integrator".to_string(),
        control_ref: refs.control.to_string(),
        canonical_prefix: refs.canonical_prefix.to_string(),
        archive_prefix: refs.archive_prefix.to_string(),
        inbox_prefix: refs.inbox_prefix.to_string(),
    };
    let facts = ProviderProtectionFacts {
        repository_id: "repo",
        policy_id: "policy",
        policy_bytes: policy,
        integrator_id: "integrator",
        control_ref: refs.control,
        canonical_prefix: refs.canonical_prefix,
        archive_prefix: refs.archive_prefix,
        inbox_prefix: refs.inbox_prefix,
        control_head,
        control_protected: true,
        canonical_create_only: true,
        archives_create_only: true,
        inboxes_writer_scoped: true,
    };
    validate_protection(Some(&marker), Some(&facts)).expect("valid protection")
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
