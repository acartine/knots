use std::cell::{Cell, RefCell};

use rusqlite::Connection;
use sha2::{Digest, Sha256};

use super::*;
use crate::compaction::{
    validate_protection, ProtectionMarker, ProviderProtectionFacts, V2RefLayout,
};
use crate::db::{
    assign_pending_outbox, inventory_legacy_files, list_pending_outbox, mark_outbox_proposed,
    record_legacy_quarantine, record_outbox_event, rotate_writer_epoch, LegacyQuarantineRecord,
};

const OID_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[derive(Default)]
struct FakeTransport {
    remote_oid: RefCell<Option<String>>,
    publish_calls: Cell<usize>,
    submitted_generations: RefCell<Vec<Option<String>>>,
    fail_after_remote_update: Cell<bool>,
    skip_remote_update: Cell<bool>,
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
            proposal_ref: V2RefLayout::default().proposal(
                &writer.writer_id,
                events[0].sequence.expect("assigned sequence"),
            ),
            proposed_oid: format!("{:x}", digest.finalize()),
            expected_inbox_oid: writer.expected_inbox_oid.clone(),
            expected_control_oid: expected_control_oid.map(str::to_string),
            includes_control_registration: !writer.registered,
            event_ids: events.iter().map(|event| event.event_id.clone()).collect(),
        })
    }

    fn submit_proposal(
        &self,
        prepared: &PreparedInbox,
        submission: &SignedSubmission,
    ) -> Result<(), SyncError> {
        self.publish_calls.set(self.publish_calls.get() + 1);
        self.submitted_generations
            .borrow_mut()
            .push(submission.bundle.base_generation.clone());
        if !self.skip_remote_update.get() {
            *self.remote_oid.borrow_mut() = Some(prepared.proposed_oid.clone());
        }
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
        .publish_pending("credential-a", &protection(Some("control-a")), None, 10)
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
        .publish_pending("credential-a", &protection(None), None, 10)
        .is_err());
    let recovered = publisher
        .publish_pending("credential-a", &protection(None), None, 10)
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
    assert_eq!(first.writer_id.len(), 64);
    assert!(first.writer_id.bytes().all(|byte| byte.is_ascii_hexdigit()));
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
fn one_signed_submission_sequence_covers_the_whole_assigned_batch() {
    let conn = test_connection();
    record(&conn, "event-a", b"a");
    record(&conn, "event-b", b"b");
    let writer = ensure_writer_epoch(&conn, "credential-a").expect("writer");
    let batch = assign_pending_outbox(&conn, &writer, 10).expect("assign batch");

    assert_eq!(batch.len(), 2);
    assert!(batch.iter().all(|event| event.sequence == Some(1)));
    let next: i64 = conn
        .query_row(
            "SELECT next_sequence FROM v2_writer_epoch WHERE writer_id = ?1",
            [&writer.writer_id],
            |row| row.get(0),
        )
        .expect("next sequence");
    assert_eq!(next, 2);
}

#[test]
fn v22_pending_receipts_migrate_without_loss_and_resequence_as_one_batch() {
    let ws = knots_test_support::workspace("signed-batch-migration");
    let path = ws.path().join("state.sqlite");
    let conn = crate::db::open_connection(path.to_str().unwrap()).expect("open database");
    record(&conn, "event-a", b"a");
    record(&conn, "event-b", b"b");
    let writer = ensure_writer_epoch(&conn, "credential-a").expect("writer");
    assign_pending_outbox(&conn, &writer, 10).expect("assign pre-migration rows");
    conn.execute(
        "UPDATE v2_outbox SET sequence = 2 WHERE event_id = 'event-b'",
        [],
    )
    .expect("model old per-event sequence");
    recovery_tests::downgrade_outbox_to_v22(&conn);
    drop(conn);
    let reopened = crate::db::open_connection(path.to_str().unwrap()).expect("migrate database");
    let preserved: i64 = reopened
        .query_row(
            "SELECT COUNT(*) FROM v2_outbox
             WHERE acknowledged_at IS NULL AND writer_id IS NULL AND sequence IS NULL",
            [],
            |row| row.get(0),
        )
        .expect("preserved receipt count");
    assert_eq!(preserved, 2);
    let writer = ensure_writer_epoch(&reopened, "credential-a").expect("reopen writer");
    let batch = assign_pending_outbox(&reopened, &writer, 10).expect("reassign batch");
    assert_eq!(batch.len(), 2);
    assert!(batch.iter().all(|event| event.sequence == Some(1)));
}

#[test]
fn v22_proposed_receipt_migration_preserves_lost_ack_recovery() {
    let ws = knots_test_support::workspace("signed-proposal-migration");
    let path = ws.path().join("state.sqlite");
    let conn = crate::db::open_connection(path.to_str().unwrap()).expect("open database");
    record(&conn, "event-a", b"a");
    let writer = ensure_writer_epoch(&conn, "credential-a").expect("writer");
    let events = assign_pending_outbox(&conn, &writer, 1).expect("assign receipt");
    mark_outbox_proposed(
        &conn,
        &writer.writer_id,
        &[events[0].event_id.clone()],
        OID_A,
        None,
    )
    .expect("record remote proposal");
    recovery_tests::downgrade_outbox_to_v22(&conn);
    drop(conn);

    let reopened = crate::db::open_connection(path.to_str().unwrap()).expect("migrate database");
    let preserved: (String, i64, String) = reopened
        .query_row(
            "SELECT writer_id, sequence, proposed_inbox_oid FROM v2_outbox",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("preserved proposal");
    assert_eq!(preserved, (writer.writer_id.clone(), 1, OID_A.to_string()));
    let next: i64 = reopened
        .query_row(
            "SELECT next_sequence FROM v2_writer_epoch WHERE writer_id = ?1",
            [&writer.writer_id],
            |row| row.get(0),
        )
        .expect("preserved next sequence");
    assert_eq!(next, 2);
    let transport = FakeTransport::default();
    *transport.remote_oid.borrow_mut() = Some(OID_A.to_string());
    let recovered = OutboxPublisher::new(&reopened, &transport)
        .publish_pending("credential-a", &protection(None), None, 10)
        .expect("recover published proposal")
        .expect("acknowledge preserved proposal");
    assert_eq!(recovered.acknowledged_events, 1);
    assert_eq!(transport.publish_calls.get(), 0);
}

#[test]
fn duplicate_receipts_are_idempotent_and_empty_publish_is_a_noop() {
    let conn = test_connection();
    record(&conn, "same-id", b"same");
    record(&conn, "same-id", b"same");
    let transport = FakeTransport::default();
    let publisher = OutboxPublisher::new(&conn, &transport);
    publisher
        .publish_pending("credential-a", &protection(None), None, 10)
        .expect("initial publish");
    assert!(publisher
        .publish_pending("credential-a", &protection(None), None, 10)
        .expect("empty publish")
        .is_none());
}

#[test]
fn credential_rotation_rejects_pending_rows_then_rotates_when_clear() {
    let conn = test_connection();
    record(&conn, "pending", b"pending");
    let writer = ensure_writer_epoch(&conn, "credential-a").expect("writer");
    assign_pending_outbox(&conn, &writer, 10).expect("assign pending");

    assert!(ensure_writer_epoch(&conn, "credential-b").is_err());
    conn.execute("DELETE FROM v2_outbox", [])
        .expect("clear pending");
    let rotated = ensure_writer_epoch(&conn, "credential-b").expect("rotate writer");
    assert_ne!(writer.writer_id, rotated.writer_id);
}

#[test]
fn proposed_oid_updates_require_exact_pending_writer_rows() {
    let conn = test_connection();
    record(&conn, "pending", b"pending");
    let writer = ensure_writer_epoch(&conn, "credential-a").expect("writer");
    assign_pending_outbox(&conn, &writer, 10).expect("assign pending");
    let ids = vec!["pending".to_string()];

    mark_outbox_proposed(&conn, &writer.writer_id, &ids, "oid-a", None).expect("first proposal");
    assert!(mark_outbox_proposed(&conn, &writer.writer_id, &ids, "oid-b", None).is_err());
    assert!(mark_outbox_proposed(&conn, "wrong-writer", &ids, "oid-a", None).is_err());
}

#[test]
fn quarantine_inventory_is_idempotent_and_hash_changes_fail_closed() {
    let conn = test_connection();
    let mut record = LegacyQuarantineRecord {
        relative_path: "events/legacy.json".to_string(),
        content_sha256: "hash-a".to_string(),
    };
    record_legacy_quarantine(&conn, &record).expect("first inventory");
    record_legacy_quarantine(&conn, &record).expect("idempotent inventory");
    record.content_sha256 = "hash-b".to_string();
    assert!(record_legacy_quarantine(&conn, &record).is_err());
}

#[test]
fn stored_proposals_must_be_uniform_and_match_the_remote() {
    let conn = test_connection();
    record(&conn, "event-a", b"a");
    record(&conn, "event-b", b"b");
    let writer = ensure_writer_epoch(&conn, "credential-a").expect("writer");
    assign_pending_outbox(&conn, &writer, 10).expect("assign pending");
    conn.execute("UPDATE v2_outbox SET proposed_inbox_oid = event_id", [])
        .expect("set mixed proposals");
    let transport = FakeTransport::default();
    assert!(OutboxPublisher::new(&conn, &transport)
        .publish_pending("credential-a", &protection(None), None, 10)
        .is_err());

    conn.execute("UPDATE v2_outbox SET proposed_inbox_oid = 'same-oid'", [])
        .expect("set uniform proposal");
    assert!(OutboxPublisher::new(&conn, &transport)
        .publish_pending("credential-a", &protection(None), None, 10)
        .is_err());
}

#[test]
fn publication_without_exact_remote_oid_remains_pending() {
    let conn = test_connection();
    record(&conn, "event-a", b"a");
    let transport = FakeTransport::default();
    transport.skip_remote_update.set(true);

    assert!(OutboxPublisher::new(&conn, &transport)
        .publish_pending("credential-a", &protection(None), None, 10)
        .is_err());
    let pending: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM v2_outbox WHERE acknowledged_at IS NULL",
            [],
            |row| row.get(0),
        )
        .expect("pending count");
    assert_eq!(pending, 1);
}

#[test]
fn malformed_writer_and_prepared_inputs_fail_closed() {
    let conn = test_connection();
    record(&conn, "event-a", b"a");
    let writer = ensure_writer_epoch(&conn, "credential-a").expect("writer");
    let events = assign_pending_outbox(&conn, &writer, 10).expect("assign pending");
    conn.execute(
        "UPDATE v2_writer_epoch SET credential_id = '' WHERE writer_id = ?1",
        [&writer.writer_id],
    )
    .expect("corrupt writer scope");
    assert!(OutboxPublisher::new(&conn, &FakeTransport::default())
        .publish_pending("credential-a", &protection(None), None, 10)
        .is_err());

    let prepared = PreparedInbox {
        writer_id: "wrong-writer".to_string(),
        inbox_ref: writer.inbox_ref.clone(),
        proposal_ref: "refs/heads/knots-v2-proposals/wrong/1".to_string(),
        proposed_oid: String::new(),
        expected_inbox_oid: writer.expected_inbox_oid.clone(),
        expected_control_oid: None,
        includes_control_registration: true,
        event_ids: events.iter().map(|event| event.event_id.clone()).collect(),
    };
    let submission =
        sign_submission(&conn, "repo", &writer, &events[..1], OID_A, None).expect("sign fixture");
    assert!(validate_prepared(&writer, &events, &submission, None, &prepared).is_err());
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
        .publish_pending("credential-a", &protection(None), None, 10)
        .expect("clone A publish")
        .expect("clone A batch");
    let result_b = OutboxPublisher::new(&clone_b, &transport_b)
        .publish_pending("credential-b", &protection(None), None, 10)
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

#[path = "outbox_recovery_tests.rs"]
mod recovery_tests;
