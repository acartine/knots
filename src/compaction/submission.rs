use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::db::{
    ensure_writer_identity_for_credential, load_writer_signing_key, OutboxRecord, WriterEpoch,
};

use super::{
    validate_inbox, InboxBundle, InboxCandidate, InboxEntry, InboxError, V2RefLayout,
    ValidatedInbox, INBOX_DATA_ROOT,
};

const DOMAIN: &[u8] = b"knots.protocol-v2.github-submission.v1";
const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SignedSubmission {
    pub schema_version: u32,
    pub repository_id: String,
    pub proposal_ref: String,
    pub proposal_oid: String,
    pub target_ref: String,
    pub expected_old_oid: Option<String>,
    pub public_key: String,
    pub parent_writer_id: Option<String>,
    pub bundle: InboxBundle,
    pub signature: String,
    pub rotation_signature: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct SubmissionCandidate {
    pub repository_id: String,
    pub proposal_ref: String,
    pub observed_oid: String,
    pub current_inbox_oid: Option<String>,
    pub expected_sequence: u64,
    pub submission: Vec<u8>,
    pub objects: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Clone)]
pub(crate) enum RegistrationAuthority<'a> {
    Existing {
        public_key: &'a [u8; 32],
    },
    First,
    Rotation {
        parent_writer_id: &'a str,
        parent_public_key: &'a [u8; 32],
    },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct PromotionRequest {
    pub proposal_ref: String,
    pub proposal_oid: String,
    pub inbox_ref: String,
    pub expected_old_oid: Option<String>,
    pub writer_id: String,
    pub sequence: u64,
    pub registration: VerifiedRegistration,
    pub inbox: ValidatedInbox,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum VerifiedRegistration {
    Existing {
        public_key: [u8; 32],
    },
    First {
        public_key: [u8; 32],
    },
    Rotation {
        public_key: [u8; 32],
        parent_writer_id: String,
    },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum SubmissionError {
    Database,
    InvalidEnvelope,
    InvalidRepository,
    InvalidProposal,
    InvalidWriter,
    InvalidKey,
    InvalidSignature,
    InvalidRotation,
    InvalidSequence,
    InvalidHead,
    InvalidEntries,
    Inbox(InboxError),
}

impl fmt::Display for SubmissionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid signed protocol-v2 submission: {self:?}")
    }
}

impl Error for SubmissionError {}

pub(crate) fn sign_submission(
    conn: &Connection,
    repository_id: &str,
    writer: &WriterEpoch,
    events: &[OutboxRecord],
    proposal_oid: &str,
    base_generation: Option<&str>,
) -> Result<SignedSubmission, SubmissionError> {
    validate_repository(repository_id)?;
    if !valid_object_id(proposal_oid) {
        return Err(SubmissionError::InvalidProposal);
    }
    let identity = ensure_writer_identity_for_credential(conn, Some(&writer.credential_id))
        .map_err(|_| SubmissionError::Database)?;
    if identity.writer_id != writer.writer_id {
        return Err(SubmissionError::InvalidWriter);
    }
    let sequence = single_sequence(events)?;
    let entries = signed_entries(events)?;
    let layout = V2RefLayout::default();
    let mut submission = SignedSubmission {
        schema_version: SCHEMA_VERSION,
        repository_id: repository_id.to_string(),
        proposal_ref: layout.proposal(&writer.writer_id, sequence),
        proposal_oid: proposal_oid.to_string(),
        target_ref: writer.inbox_ref.clone(),
        expected_old_oid: writer.expected_inbox_oid.clone(),
        public_key: hex_encode(&identity.public_key),
        parent_writer_id: identity.parent_writer_id.clone(),
        bundle: InboxBundle {
            schema_version: 1,
            writer_id: writer.writer_id.clone(),
            sequence,
            previous_head: writer.expected_inbox_oid.clone(),
            base_generation: base_generation.map(str::to_string),
            entries,
        },
        signature: String::new(),
        rotation_signature: None,
    };
    let transcript = transcript(&submission)?;
    let signing =
        load_writer_signing_key(conn, &writer.writer_id).map_err(|_| SubmissionError::Database)?;
    submission.signature = hex_encode(&signing.sign(&transcript).to_bytes());
    if let Some(parent) = &identity.parent_writer_id {
        let parent_key =
            load_writer_signing_key(conn, parent).map_err(|_| SubmissionError::Database)?;
        submission.rotation_signature = Some(hex_encode(&parent_key.sign(&transcript).to_bytes()));
    }
    Ok(submission)
}

pub(crate) fn verify_submission(
    candidate: &SubmissionCandidate,
    authority: RegistrationAuthority<'_>,
) -> Result<PromotionRequest, SubmissionError> {
    let submission: SignedSubmission = serde_json::from_slice(&candidate.submission)
        .map_err(|_| SubmissionError::InvalidEnvelope)?;
    validate_shape(candidate, &submission)?;
    let public = decode_array::<32>(&submission.public_key).ok_or(SubmissionError::InvalidKey)?;
    let fingerprint = format!("{:x}", Sha256::digest(public));
    if fingerprint != submission.bundle.writer_id {
        return Err(SubmissionError::InvalidWriter);
    }
    let registration = validate_registration(&submission, authority, &public)?;
    let transcript = transcript(&submission)?;
    verify_signature(&public, &submission.signature, &transcript)?;
    let bundle =
        serde_json::to_vec(&submission.bundle).map_err(|_| SubmissionError::InvalidEnvelope)?;
    let inbox = validate_inbox(&InboxCandidate {
        inbox_ref: submission.target_ref.clone(),
        expected_head: candidate.observed_oid.clone(),
        observed_head: candidate.observed_oid.clone(),
        bundle,
        objects: candidate.objects.clone(),
    })
    .map_err(SubmissionError::Inbox)?;
    Ok(PromotionRequest {
        proposal_ref: submission.proposal_ref,
        proposal_oid: candidate.observed_oid.clone(),
        inbox_ref: submission.target_ref,
        expected_old_oid: submission.expected_old_oid,
        writer_id: inbox.writer_id.clone(),
        sequence: inbox.sequence,
        registration,
        inbox,
    })
}

fn validate_shape(
    candidate: &SubmissionCandidate,
    submission: &SignedSubmission,
) -> Result<(), SubmissionError> {
    validate_repository(&submission.repository_id)?;
    if submission.schema_version != SCHEMA_VERSION
        || submission.repository_id != candidate.repository_id
    {
        return Err(SubmissionError::InvalidRepository);
    }
    let layout = V2RefLayout::default();
    let expected_proposal =
        layout.proposal(&submission.bundle.writer_id, submission.bundle.sequence);
    if submission.proposal_ref != expected_proposal
        || candidate.proposal_ref != expected_proposal
        || submission.proposal_oid != candidate.observed_oid
        || !valid_object_id(&candidate.observed_oid)
    {
        return Err(SubmissionError::InvalidProposal);
    }
    if submission.target_ref != layout.inbox(&submission.bundle.writer_id) {
        return Err(SubmissionError::InvalidWriter);
    }
    if submission.bundle.sequence != candidate.expected_sequence || submission.bundle.sequence == 0
    {
        return Err(SubmissionError::InvalidSequence);
    }
    if submission.expected_old_oid != candidate.current_inbox_oid
        || submission.bundle.previous_head != submission.expected_old_oid
        || submission
            .expected_old_oid
            .as_deref()
            .is_some_and(|oid| !valid_object_id(oid))
    {
        return Err(SubmissionError::InvalidHead);
    }
    if submission.bundle.entries.is_empty()
        || !submission
            .bundle
            .entries
            .windows(2)
            .all(|pair| pair[0].path < pair[1].path)
    {
        return Err(SubmissionError::InvalidEntries);
    }
    Ok(())
}

fn validate_registration(
    submission: &SignedSubmission,
    authority: RegistrationAuthority<'_>,
    public: &[u8; 32],
) -> Result<VerifiedRegistration, SubmissionError> {
    match authority {
        RegistrationAuthority::Existing { public_key } => {
            if public_key != public
                || submission.parent_writer_id.is_some() && submission.rotation_signature.is_none()
            {
                return Err(SubmissionError::InvalidKey);
            }
            Ok(VerifiedRegistration::Existing {
                public_key: *public,
            })
        }
        RegistrationAuthority::First => {
            if submission.bundle.sequence != 1
                || submission.expected_old_oid.is_some()
                || submission.parent_writer_id.is_some()
                || submission.rotation_signature.is_some()
            {
                return Err(SubmissionError::InvalidRotation);
            }
            Ok(VerifiedRegistration::First {
                public_key: *public,
            })
        }
        RegistrationAuthority::Rotation {
            parent_writer_id,
            parent_public_key,
        } => {
            if submission.bundle.sequence != 1
                || submission.expected_old_oid.is_some()
                || submission.parent_writer_id.as_deref() != Some(parent_writer_id)
            {
                return Err(SubmissionError::InvalidRotation);
            }
            let signature = submission
                .rotation_signature
                .as_deref()
                .ok_or(SubmissionError::InvalidRotation)?;
            verify_signature(parent_public_key, signature, &transcript(submission)?)
                .map_err(|_| SubmissionError::InvalidRotation)?;
            Ok(VerifiedRegistration::Rotation {
                public_key: *public,
                parent_writer_id: parent_writer_id.to_string(),
            })
        }
    }
}

fn transcript(submission: &SignedSubmission) -> Result<Vec<u8>, SubmissionError> {
    let mut bytes = Vec::new();
    put_bytes(&mut bytes, DOMAIN);
    put_u64(&mut bytes, submission.schema_version as u64);
    for value in [
        &submission.repository_id,
        &submission.proposal_ref,
        &submission.proposal_oid,
        &submission.target_ref,
        &submission.bundle.writer_id,
        &submission.public_key,
    ] {
        put_bytes(&mut bytes, value.as_bytes());
    }
    put_optional(&mut bytes, submission.expected_old_oid.as_deref());
    put_optional(&mut bytes, submission.parent_writer_id.as_deref());
    put_u64(&mut bytes, submission.bundle.schema_version as u64);
    put_u64(&mut bytes, submission.bundle.sequence);
    put_optional(&mut bytes, submission.bundle.previous_head.as_deref());
    put_optional(&mut bytes, submission.bundle.base_generation.as_deref());
    put_u64(&mut bytes, submission.bundle.entries.len() as u64);
    for entry in &submission.bundle.entries {
        put_bytes(&mut bytes, entry.path.as_bytes());
        put_bytes(&mut bytes, entry.event_id.as_bytes());
        put_bytes(&mut bytes, entry.content_sha256.as_bytes());
        put_u64(&mut bytes, entry.bytes);
    }
    Ok(bytes)
}

fn signed_entries(events: &[OutboxRecord]) -> Result<Vec<InboxEntry>, SubmissionError> {
    let mut entries = events
        .iter()
        .map(|event| InboxEntry {
            path: format!("{INBOX_DATA_ROOT}/{}", event.relative_path),
            event_id: event.event_id.clone(),
            content_sha256: event.content_sha256.clone(),
            bytes: event.payload.len() as u64,
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    if entries.is_empty() {
        return Err(SubmissionError::InvalidEntries);
    }
    Ok(entries)
}

fn single_sequence(events: &[OutboxRecord]) -> Result<u64, SubmissionError> {
    let sequence = events
        .first()
        .and_then(|event| event.sequence)
        .filter(|sequence| *sequence > 0)
        .ok_or(SubmissionError::InvalidSequence)?;
    events
        .iter()
        .all(|event| event.sequence == Some(sequence))
        .then_some(sequence)
        .ok_or(SubmissionError::InvalidSequence)
}

fn verify_signature(
    public: &[u8; 32],
    encoded: &str,
    message: &[u8],
) -> Result<(), SubmissionError> {
    let signature = decode_array::<64>(encoded).ok_or(SubmissionError::InvalidSignature)?;
    let public = VerifyingKey::from_bytes(public).map_err(|_| SubmissionError::InvalidKey)?;
    public
        .verify_strict(message, &Signature::from_bytes(&signature))
        .map_err(|_| SubmissionError::InvalidSignature)
}

fn validate_repository(value: &str) -> Result<(), SubmissionError> {
    (!value.trim().is_empty() && value.len() <= 256 && !value.contains(['\n', '\r']))
        .then_some(())
        .ok_or(SubmissionError::InvalidRepository)
}

fn valid_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn put_bytes(output: &mut Vec<u8>, value: &[u8]) {
    put_u64(output, value.len() as u64);
    output.extend_from_slice(value);
}

fn put_optional(output: &mut Vec<u8>, value: Option<&str>) {
    output.push(u8::from(value.is_some()));
    if let Some(value) = value {
        put_bytes(output, value.as_bytes());
    }
}

fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

pub(super) fn decode_array<const N: usize>(value: &str) -> Option<[u8; N]> {
    if value.len() != N * 2 {
        return None;
    }
    let mut output = [0_u8; N];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        output[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
    }
    Some(output)
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
#[path = "submission_tests.rs"]
mod tests;
