// The provider-specific transport is wired by the receive-control rollout knots.
#![allow(dead_code)]

use rusqlite::Connection;

use crate::compaction::{V2RefLayout, ValidatedProtection};
use crate::db::{
    acknowledge_outbox, assign_pending_outbox, ensure_writer_epoch, mark_outbox_proposed,
    OutboxRecord, WriterEpoch,
};
use crate::sync::SyncError;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct PreparedInbox {
    pub writer_id: String,
    pub inbox_ref: String,
    pub proposed_oid: String,
    pub expected_inbox_oid: Option<String>,
    pub expected_control_oid: Option<String>,
    pub includes_control_registration: bool,
    pub event_ids: Vec<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct PublishedInbox {
    pub writer_id: String,
    pub inbox_oid: String,
    pub acknowledged_events: usize,
    pub registered_writer: bool,
}

pub(crate) trait InboxTransport {
    /// Prepare the exact immutable commit locally without contacting the remote.
    fn prepare(
        &self,
        writer: &WriterEpoch,
        events: &[OutboxRecord],
        expected_control_oid: Option<&str>,
    ) -> Result<PreparedInbox, SyncError>;

    /// Publish with exact leases. First publication atomically updates control and inbox.
    fn publish(&self, prepared: &PreparedInbox) -> Result<(), SyncError>;

    /// Fetch the credential-scoped inbox ref after publication or a lost response.
    fn fetch_inbox_oid(&self, inbox_ref: &str) -> Result<Option<String>, SyncError>;
}

pub(crate) struct OutboxPublisher<'a, T> {
    conn: &'a Connection,
    transport: &'a T,
}

impl<'a, T: InboxTransport> OutboxPublisher<'a, T> {
    pub(crate) fn new(conn: &'a Connection, transport: &'a T) -> Self {
        Self { conn, transport }
    }

    pub(crate) fn publish_pending(
        &self,
        credential_id: &str,
        protection: &ValidatedProtection,
        limit: usize,
    ) -> Result<Option<PublishedInbox>, SyncError> {
        require_protection_identity(protection)?;
        let writer = ensure_writer_epoch(self.conn, credential_id)?;
        require_writer_scope(&writer)?;
        let events = assign_pending_outbox(self.conn, &writer, limit)?;
        if events.is_empty() {
            return Ok(None);
        }
        if let Some(recovered) = self.confirm_prepared(&writer, &events)? {
            return Ok(Some(recovered));
        }

        let expected_control = (!writer.registered)
            .then(|| protection.control_head())
            .flatten();
        let prepared = self.transport.prepare(&writer, &events, expected_control)?;
        validate_prepared(&writer, &events, expected_control, &prepared)?;
        mark_outbox_proposed(
            self.conn,
            &writer.writer_id,
            &prepared.event_ids,
            &prepared.proposed_oid,
        )?;
        self.transport.publish(&prepared)?;
        self.confirm_exact_oid(&writer, &prepared.proposed_oid, !writer.registered)
            .map(Some)
    }

    fn confirm_prepared(
        &self,
        writer: &WriterEpoch,
        events: &[OutboxRecord],
    ) -> Result<Option<PublishedInbox>, SyncError> {
        let proposed = events
            .first()
            .and_then(|event| event.proposed_inbox_oid.as_deref());
        let Some(proposed) = proposed else {
            return Ok(None);
        };
        if events
            .iter()
            .any(|event| event.proposed_inbox_oid.as_deref() != Some(proposed))
        {
            return Err(outbox_error(
                "pending batch contains mixed proposed inbox OIDs",
            ));
        }
        let remote = self.transport.fetch_inbox_oid(&writer.inbox_ref)?;
        if remote.as_deref() != Some(proposed) {
            return Err(outbox_error(
                "stored proposed inbox OID does not match the remote inbox",
            ));
        }
        self.confirm_exact_oid(writer, proposed, !writer.registered)
            .map(Some)
    }

    fn confirm_exact_oid(
        &self,
        writer: &WriterEpoch,
        proposed_oid: &str,
        registered: bool,
    ) -> Result<PublishedInbox, SyncError> {
        let remote = self.transport.fetch_inbox_oid(&writer.inbox_ref)?;
        if remote.as_deref() != Some(proposed_oid) {
            return Err(outbox_error(
                "remote inbox OID was not confirmed; outbox remains pending",
            ));
        }
        let acknowledged =
            acknowledge_outbox(self.conn, &writer.writer_id, proposed_oid, registered)?;
        Ok(PublishedInbox {
            writer_id: writer.writer_id.clone(),
            inbox_oid: proposed_oid.to_string(),
            acknowledged_events: acknowledged,
            registered_writer: registered,
        })
    }
}

fn require_protection_identity(protection: &ValidatedProtection) -> Result<(), SyncError> {
    if protection.repository_id().is_empty() || protection.integrator_id().is_empty() {
        return Err(outbox_error("provider protection identity is empty"));
    }
    Ok(())
}

fn require_writer_scope(writer: &WriterEpoch) -> Result<(), SyncError> {
    let expected = V2RefLayout::default().inbox(&writer.writer_id);
    if writer.inbox_ref != expected || writer.credential_id.trim().is_empty() {
        return Err(outbox_error("writer epoch is not credential scoped"));
    }
    Ok(())
}

fn validate_prepared(
    writer: &WriterEpoch,
    events: &[OutboxRecord],
    expected_control: Option<&str>,
    prepared: &PreparedInbox,
) -> Result<(), SyncError> {
    let event_ids: Vec<_> = events.iter().map(|event| event.event_id.clone()).collect();
    let valid = prepared.writer_id == writer.writer_id
        && prepared.inbox_ref == writer.inbox_ref
        && prepared.expected_inbox_oid == writer.expected_inbox_oid
        && prepared.expected_control_oid.as_deref() == expected_control
        && prepared.includes_control_registration != writer.registered
        && prepared.event_ids == event_ids
        && !prepared.proposed_oid.is_empty();
    if !valid {
        return Err(outbox_error(
            "prepared inbox does not preserve exact writer, event, or lease inputs",
        ));
    }
    Ok(())
}

fn outbox_error(message: &str) -> SyncError {
    SyncError::Compaction {
        message: format!("protocol-v2 outbox: {message}"),
    }
}

#[cfg(test)]
#[path = "outbox_tests.rs"]
mod tests;
