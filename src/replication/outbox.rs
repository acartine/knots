// The provider-specific transport is wired by the receive-control rollout knots.
#![allow(dead_code)]

use crate::compaction::{sign_submission, SignedSubmission, V2RefLayout, ValidatedProtection};
use crate::db::{
    acknowledge_outbox, assign_pending_outbox, ensure_writer_epoch, mark_outbox_proposed,
    OutboxRecord, WriterEpoch,
};
use crate::sync::SyncError;
use rusqlite::Connection;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct PreparedInbox {
    pub writer_id: String,
    pub inbox_ref: String,
    pub proposal_ref: String,
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

    /// Publish only the immutable untrusted proposal ref. A trusted Action promotes it.
    fn submit_proposal(
        &self,
        prepared: &PreparedInbox,
        submission: &SignedSubmission,
    ) -> Result<(), SyncError>;

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
        base_generation: Option<&str>,
        limit: usize,
    ) -> Result<Option<PublishedInbox>, SyncError> {
        require_protection_identity(protection)?;
        let writer = ensure_writer_epoch(self.conn, credential_id)?;
        require_writer_scope(&writer)?;
        let events = assign_pending_outbox(self.conn, &writer, limit)?;
        if events.is_empty() {
            return Ok(None);
        }
        let expected_control = (!writer.registered)
            .then(|| protection.control_head())
            .flatten();
        if let Some(recovered) =
            self.confirm_prepared(&writer, &events, protection, expected_control)?
        {
            return Ok(Some(recovered));
        }
        let prepared = self.transport.prepare(&writer, &events, expected_control)?;
        let submission = sign_submission(
            self.conn,
            protection.repository_id(),
            &writer,
            &events,
            &prepared.proposed_oid,
            base_generation,
        )
        .map_err(|error| outbox_error(&error.to_string()))?;
        validate_prepared(&writer, &events, &submission, expected_control, &prepared)?;
        mark_outbox_proposed(
            self.conn,
            &writer.writer_id,
            &prepared.event_ids,
            &prepared.proposed_oid,
            base_generation,
        )?;
        self.transport.submit_proposal(&prepared, &submission)?;
        self.confirm_exact_oid(&writer, &prepared.proposed_oid, !writer.registered)
            .map(Some)
    }

    fn confirm_prepared(
        &self,
        writer: &WriterEpoch,
        events: &[OutboxRecord],
        protection: &ValidatedProtection,
        expected_control: Option<&str>,
    ) -> Result<Option<PublishedInbox>, SyncError> {
        let proposed = events
            .first()
            .and_then(|event| event.proposed_inbox_oid.as_deref());
        let Some(proposed) = proposed else {
            return Ok(None);
        };
        let durable_generation = events[0].proposal_base_generation.as_deref();
        if events.iter().any(|event| {
            event.proposed_inbox_oid.as_deref() != Some(proposed)
                || event.proposal_base_generation.as_deref() != durable_generation
        }) {
            return Err(outbox_error(
                "pending batch contains mixed proposed inbox OIDs",
            ));
        }
        let remote = self.transport.fetch_inbox_oid(&writer.inbox_ref)?;
        if remote.as_deref() == Some(proposed) {
            return self
                .confirm_exact_oid(writer, proposed, !writer.registered)
                .map(Some);
        }
        if remote != writer.expected_inbox_oid {
            return Err(outbox_error(
                "stored proposal conflicts with the current remote inbox",
            ));
        }
        let prepared = self.transport.prepare(writer, events, expected_control)?;
        let submission = sign_submission(
            self.conn,
            protection.repository_id(),
            writer,
            events,
            proposed,
            durable_generation,
        )
        .map_err(|error| outbox_error(&error.to_string()))?;
        validate_prepared(writer, events, &submission, expected_control, &prepared)?;
        if prepared.proposed_oid != proposed {
            return Err(outbox_error(
                "reconstructed proposal OID differs from the durable proposal",
            ));
        }
        self.transport.submit_proposal(&prepared, &submission)?;
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
    submission: &SignedSubmission,
    expected_control: Option<&str>,
    prepared: &PreparedInbox,
) -> Result<(), SyncError> {
    let event_ids: Vec<_> = events.iter().map(|event| event.event_id.clone()).collect();
    let valid = prepared.writer_id == writer.writer_id
        && prepared.inbox_ref == writer.inbox_ref
        && prepared.proposal_ref == submission.proposal_ref
        && prepared.proposed_oid == submission.proposal_oid
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
