use super::*;

pub(super) fn downgrade_outbox_to_v22(conn: &Connection) {
    conn.execute_batch(
        "DROP INDEX idx_v2_outbox_pending;
         ALTER TABLE v2_outbox RENAME TO v2_outbox_v23;
         CREATE TABLE v2_outbox (
             event_id TEXT PRIMARY KEY, stream TEXT NOT NULL,
             relative_path TEXT NOT NULL UNIQUE, content_sha256 TEXT NOT NULL,
             payload BLOB NOT NULL, writer_id TEXT, sequence INTEGER,
             proposed_inbox_oid TEXT, acknowledged_inbox_oid TEXT,
             created_at TEXT NOT NULL, acknowledged_at TEXT,
             FOREIGN KEY(writer_id) REFERENCES v2_writer_epoch(writer_id),
             UNIQUE(writer_id, sequence)
         );
         INSERT INTO v2_outbox (
             event_id, stream, relative_path, content_sha256, payload, writer_id,
             sequence, proposed_inbox_oid, acknowledged_inbox_oid, created_at,
             acknowledged_at
         )
         SELECT event_id, stream, relative_path, content_sha256, payload, writer_id,
                sequence, proposed_inbox_oid, acknowledged_inbox_oid, created_at,
                acknowledged_at
         FROM v2_outbox_v23;
         DROP TABLE v2_outbox_v23;
         CREATE INDEX idx_v2_outbox_pending
         ON v2_outbox(acknowledged_at, created_at, event_id);
         DELETE FROM schema_migrations WHERE version = 23;
         UPDATE meta SET value = '22' WHERE key = 'schema_version';",
    )
    .expect("downgrade fixture to v22");
}

#[test]
fn failed_action_invocation_reconstructs_and_resubmits_the_signed_proposal() {
    let conn = test_connection();
    record(&conn, "local-a", b"a");
    let transport = FakeTransport::default();
    transport.skip_remote_update.set(true);
    let publisher = OutboxPublisher::new(&conn, &transport);

    assert!(publisher
        .publish_pending("credential-a", &protection(None), Some("gen-a"), 10)
        .is_err());
    transport.skip_remote_update.set(false);
    let recovered = publisher
        .publish_pending("credential-a", &protection(None), Some("gen-b"), 10)
        .expect("retry exact signed submission")
        .expect("acknowledge retried proposal");

    assert_eq!(recovered.acknowledged_events, 1);
    assert_eq!(transport.publish_calls.get(), 2);
    assert_eq!(
        *transport.submitted_generations.borrow(),
        vec![Some("gen-a".to_string()), Some("gen-a".to_string())]
    );
}
