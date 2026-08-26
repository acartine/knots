use rusqlite::Connection;
use serde_json::json;
use sha2::{Digest, Sha256};

use super::*;
use crate::db::{
    acknowledge_outbox, assign_pending_outbox, ensure_writer_epoch, mark_outbox_proposed,
    record_outbox_event, rotate_writer_epoch, OutboxRecord, WriterEpoch,
};

const OID_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OID_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[test]
fn first_submission_verifies_and_returns_an_exact_promotion() {
    let (conn, writer, events, payload) = fixture("credential-a", "event-a");
    let signed = sign_submission(
        &conn,
        "acartine/knots",
        &writer,
        &events,
        OID_A,
        Some("gen-a"),
    )
    .expect("sign submission");
    let candidate = candidate(&signed, &payload, None, 1);

    let promotion = verify_submission(&candidate, RegistrationAuthority::First)
        .expect("verify first registration");

    assert_eq!(promotion.proposal_ref, signed.proposal_ref);
    assert_eq!(promotion.proposal_oid, OID_A);
    assert_eq!(promotion.inbox_ref, writer.inbox_ref);
    assert_eq!(promotion.expected_old_oid, None);
    assert_eq!(promotion.writer_id, writer.writer_id);
    assert_eq!(promotion.sequence, 1);
    assert_eq!(
        promotion.registration,
        VerifiedRegistration::First {
            public_key: decode_array(&signed.public_key).expect("verified public key")
        }
    );
    assert_eq!(promotion.inbox.events[0].bytes, payload);
}

#[test]
fn one_signature_covers_every_event_in_a_durable_batch() {
    let conn = crate::db::open_connection(":memory:").expect("open database");
    let payload_a = record(&conn, "event-a");
    let payload_b = record(&conn, "event-b");
    let writer = ensure_writer_epoch(&conn, "credential-a").expect("ensure writer");
    let events = assign_pending_outbox(&conn, &writer, 10).expect("assign batch");
    let signed = sign_submission(&conn, "acartine/knots", &writer, &events, OID_A, None)
        .expect("sign batch");
    let mut input = candidate(&signed, &payload_a, None, 1);
    input.objects.insert(
        signed.bundle.entries[1].path.clone(),
        if signed.bundle.entries[1].event_id == "event-b" {
            payload_b
        } else {
            payload_a
        },
    );

    let promotion = verify_submission(&input, RegistrationAuthority::First).expect("verify batch");
    assert_eq!(promotion.inbox.events.len(), 2);
    assert!(signed.bundle.entries.iter().all(|entry| entry.bytes > 0));
}

#[test]
fn signature_binds_repository_refs_head_sequence_and_payload_hashes() {
    let (conn, writer, events, payload) = fixture("credential-a", "event-a");
    let signed = sign_submission(&conn, "acartine/knots", &writer, &events, OID_A, None)
        .expect("sign submission");

    let mut changed = signed.clone();
    changed.repository_id = "other/repo".to_string();
    let mut input = candidate(&changed, &payload, None, 1);
    input.repository_id = changed.repository_id.clone();
    assert!(verify_submission(&input, RegistrationAuthority::First).is_err());

    let mut changed = signed.clone();
    changed.expected_old_oid = Some(OID_B.to_string());
    changed.bundle.previous_head = Some(OID_B.to_string());
    let input = candidate(&changed, &payload, Some(OID_B), 1);
    assert!(verify_submission(&input, RegistrationAuthority::First).is_err());

    let mut changed = signed.clone();
    changed.bundle.sequence = 2;
    changed.proposal_ref = V2RefLayout::default().proposal(&writer.writer_id, 2);
    let input = candidate(&changed, &payload, None, 2);
    assert!(verify_submission(&input, RegistrationAuthority::First).is_err());

    let mut changed = signed.clone();
    changed.bundle.entries[0].content_sha256 = digest(b"different");
    let input = candidate(&changed, &payload, None, 1);
    assert!(verify_submission(&input, RegistrationAuthority::First).is_err());

    let mut changed = signed;
    changed.target_ref = "refs/heads/knots-v2-inbox/not-this-writer".to_string();
    let input = candidate(&changed, &payload, None, 1);
    assert!(verify_submission(&input, RegistrationAuthority::First).is_err());
}

#[test]
fn forged_replayed_and_cross_writer_submissions_fail_closed() {
    let (conn, writer, events, payload) = fixture("credential-a", "event-a");
    let signed = sign_submission(&conn, "acartine/knots", &writer, &events, OID_A, None)
        .expect("sign submission");
    let public = decode_array::<32>(&signed.public_key).expect("public key");
    let input = candidate(&signed, &payload, None, 1);

    let mut forged = signed.clone();
    forged.signature.replace_range(0..2, "00");
    assert!(verify_submission(
        &candidate(&forged, &payload, None, 1),
        RegistrationAuthority::First
    )
    .is_err());

    let replay = SubmissionCandidate {
        expected_sequence: 2,
        current_inbox_oid: Some(OID_B.to_string()),
        ..input.clone()
    };
    assert!(verify_submission(
        &replay,
        RegistrationAuthority::Existing {
            public_key: &public
        }
    )
    .is_err());

    let other = ed25519_dalek::SigningKey::from_bytes(&[9_u8; 32])
        .verifying_key()
        .to_bytes();
    assert!(verify_submission(
        &input,
        RegistrationAuthority::Existing { public_key: &other }
    )
    .is_err());

    let rewrapped = SubmissionCandidate {
        observed_oid: OID_B.to_string(),
        ..input.clone()
    };
    assert!(verify_submission(&rewrapped, RegistrationAuthority::First).is_err());

    let mut extra_tree_path = input;
    extra_tree_path
        .objects
        .insert("unexpected.txt".to_string(), b"unsigned".to_vec());
    assert!(verify_submission(&extra_tree_path, RegistrationAuthority::First).is_err());
}

#[test]
fn rotation_requires_both_the_parent_and_new_key_signatures() {
    let (conn, first, events, _payload) = fixture("credential-a", "event-a");
    let initial = sign_submission(&conn, "acartine/knots", &first, &events, OID_A, None)
        .expect("sign initial");
    mark_outbox_proposed(
        &conn,
        &first.writer_id,
        &[events[0].event_id.clone()],
        OID_A,
        None,
    )
    .expect("mark proposed");
    acknowledge_outbox(&conn, &first.writer_id, OID_A, true).expect("acknowledge initial");

    let second = rotate_writer_epoch(&conn, "credential-b").expect("rotate writer");
    let payload = record(&conn, "event-b");
    let events = assign_pending_outbox(&conn, &second, 1).expect("assign rotated receipt");
    let signed = sign_submission(&conn, "acartine/knots", &second, &events, OID_A, None)
        .expect("sign rotated submission");
    let parent_public = decode_array::<32>(&initial.public_key).expect("parent public key");
    let authority = RegistrationAuthority::Rotation {
        parent_writer_id: &first.writer_id,
        parent_public_key: &parent_public,
    };
    verify_submission(&candidate(&signed, &payload, None, 1), authority.clone())
        .expect("verify rotation");

    let mut forged = signed;
    forged
        .rotation_signature
        .as_mut()
        .expect("rotation signature")
        .replace_range(0..2, "00");
    assert!(verify_submission(&candidate(&forged, &payload, None, 1), authority).is_err());
}

#[test]
fn released_v0176_unsigned_data_is_not_a_submission() {
    const EVENT: &[u8] = include_bytes!("../../tests/fixtures/releases/v0.17.6/event.json");
    const PROVENANCE: &str = include_str!("../../tests/fixtures/releases/v0.17.6/provenance.json");
    assert!(PROVENANCE.contains("d67c43d165fb54a3f830a7d455ee72361f4c3a18"));
    let path = format!("{INBOX_DATA_ROOT}/events/v0176.json");
    let unsigned = InboxBundle {
        schema_version: 1,
        writer_id: "a".repeat(64),
        sequence: 1,
        previous_head: None,
        base_generation: None,
        entries: vec![InboxEntry {
            path: path.clone(),
            event_id: "v0176".to_string(),
            content_sha256: digest(EVENT),
            bytes: EVENT.len() as u64,
        }],
    };
    let candidate = SubmissionCandidate {
        repository_id: "acartine/knots".to_string(),
        proposal_ref: V2RefLayout::default().proposal(&unsigned.writer_id, 1),
        observed_oid: OID_A.to_string(),
        current_inbox_oid: None,
        expected_sequence: 1,
        submission: serde_json::to_vec(&unsigned).expect("serialize unsigned bundle"),
        objects: BTreeMap::from([(path, EVENT.to_vec())]),
    };
    assert_eq!(
        verify_submission(&candidate, RegistrationAuthority::First),
        Err(SubmissionError::InvalidEnvelope)
    );
}

#[test]
fn canonical_transcript_has_a_fixed_cross_version_vector() {
    let signing = ed25519_dalek::SigningKey::from_bytes(&[7_u8; 32]);
    let public = signing.verifying_key().to_bytes();
    let writer_id = digest(&public);
    let submission = SignedSubmission {
        schema_version: 1,
        repository_id: "acartine/knots".to_string(),
        proposal_ref: V2RefLayout::default().proposal(&writer_id, 1),
        proposal_oid: OID_A.to_string(),
        target_ref: V2RefLayout::default().inbox(&writer_id),
        expected_old_oid: None,
        public_key: hex_encode(&public),
        parent_writer_id: None,
        bundle: InboxBundle {
            schema_version: 1,
            writer_id,
            sequence: 1,
            previous_head: None,
            base_generation: Some("generation-a".to_string()),
            entries: vec![InboxEntry {
                path: ".knots/v2/inbox/events/a.json".to_string(),
                event_id: "event-a".to_string(),
                content_sha256: digest(b"payload-a"),
                bytes: 9,
            }],
        },
        signature: String::new(),
        rotation_signature: None,
    };
    let bytes = transcript(&submission).expect("canonical transcript");
    let signature = signing.sign(&bytes).to_bytes();

    assert_eq!(
        digest(&bytes),
        "5e8dcfc83955bb064e0ac15e7dac72478928a685574b33b33501959edc82f762"
    );
    assert_eq!(
        hex_encode(&signature),
        "0a9cf0cd72a90dfef0cdce4a7d47a7fbec0b25901770f57bd76817e12e756e28\
         104500adfec17285d3d21f43307b4243ebce134d707e164b7924caf7baf73804"
    );
}

fn fixture(
    credential: &str,
    event_id: &str,
) -> (Connection, WriterEpoch, Vec<OutboxRecord>, Vec<u8>) {
    let conn = crate::db::open_connection(":memory:").expect("open database");
    let payload = record(&conn, event_id);
    let writer = ensure_writer_epoch(&conn, credential).expect("ensure writer");
    let events = assign_pending_outbox(&conn, &writer, 1).expect("assign receipt");
    (conn, writer, events, payload)
}

fn record(conn: &Connection, event_id: &str) -> Vec<u8> {
    let payload = serde_json::to_vec(&json!({
        "event_id": event_id,
        "occurred_at": "2026-08-26T04:00:00Z",
        "knot_id": format!("knot-{event_id}"),
        "type": "knot.created",
        "data": {"title": event_id}
    }))
    .expect("event serializes");
    record_outbox_event(
        conn,
        event_id,
        "events",
        &format!("events/2026/08/26/{event_id}.json"),
        &digest(&payload),
        &payload,
    )
    .expect("record receipt");
    payload
}

fn candidate(
    signed: &SignedSubmission,
    payload: &[u8],
    current_inbox_oid: Option<&str>,
    expected_sequence: u64,
) -> SubmissionCandidate {
    SubmissionCandidate {
        repository_id: "acartine/knots".to_string(),
        proposal_ref: signed.proposal_ref.clone(),
        observed_oid: OID_A.to_string(),
        current_inbox_oid: current_inbox_oid.map(str::to_string),
        expected_sequence,
        submission: serde_json::to_vec(signed).expect("submission serializes"),
        objects: BTreeMap::from([(signed.bundle.entries[0].path.clone(), payload.to_vec())]),
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
