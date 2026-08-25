use sha2::{Digest, Sha256};

use super::*;

const COMMIT: &str = "1111111111111111111111111111111111111111";
const INDEX_TREE: &str = "2222222222222222222222222222222222222222";
const EVENT_TREE: &str = "3333333333333333333333333333333333333333";
const CONTROL_HEAD: &str = "4444444444444444444444444444444444444444";
const NEXT_CONTROL_HEAD: &str = "5555555555555555555555555555555555555555";
const OLD_GENERATION: &str = "v2-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn active_bytes() -> &'static [u8] {
    br#"{"schema_version":1,"hot":[{}],"warm":[{}]}"#
}

fn cold_bytes() -> &'static [u8] {
    br#"{"schema_version":1,"cold":[{}]}"#
}

fn pack_bytes() -> &'static [u8] {
    br#"{"event_id":"event-1","body":"raw bytes retained"}"#
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn descriptor(path: &str, bytes: &[u8], records: u64) -> SnapshotDescriptor {
    SnapshotDescriptor {
        path: path.to_string(),
        sha256: digest(bytes),
        bytes: bytes.len() as u64,
        records,
    }
}

fn writer(sequence: u64) -> WriterHead {
    WriterHead {
        writer_id: "writer-a".to_string(),
        inbox_ref: V2RefLayout::default().inbox("writer-a"),
        commit: COMMIT.to_string(),
        sequence,
    }
}

fn manifest(previous: Option<&str>, epoch: u64) -> CompactionManifest {
    let pack_sha = digest(pack_bytes());
    CompactionManifest {
        protocol_version: PROTOCOL_VERSION,
        generation_id: String::new(),
        predecessor_generation: previous.map(str::to_string),
        predecessor_control_epoch: epoch,
        source: SourceCheckpoint {
            legacy_ref: LEGACY_REF.to_string(),
            cutoff_commit: COMMIT.to_string(),
            index_tree: INDEX_TREE.to_string(),
            event_tree: EVENT_TREE.to_string(),
        },
        writer_heads: vec![writer(7)],
        snapshots: SnapshotSet {
            active: descriptor(
                ".knots/v2/generations/current/active.snapshot.json",
                active_bytes(),
                2,
            ),
            cold: descriptor(
                ".knots/v2/generations/current/cold.snapshot.json",
                cold_bytes(),
                1,
            ),
        },
        packs: vec![EventPack {
            pack_id: format!("pack-{pack_sha}"),
            path: format!(".knots/v2/packs/pack-{pack_sha}.pack"),
            sha256: pack_sha,
            bytes: pack_bytes().len() as u64,
            events: vec![EventIndexEntry {
                event_id: "event-1".to_string(),
                content_sha256: digest(pack_bytes()),
                offset: 0,
                bytes: pack_bytes().len() as u64,
            }],
        }],
        compatibility: Compatibility {
            minimum_reader_protocol: PROTOCOL_VERSION,
            minimum_writer_protocol: PROTOCOL_VERSION,
        },
    }
    .seal()
}

fn context<'a>(
    value: &'a CompactionManifest,
    pack: &'a [u8],
    previous: Option<&'a str>,
    epoch: u64,
) -> ValidationContext<'a> {
    let packs = Box::leak(Box::new([(value.packs[0].pack_id.as_str(), pack)]));
    ValidationContext {
        active_snapshot: Some(active_bytes()),
        cold_snapshot: Some(cold_bytes()),
        packs,
        source: SourceFacts {
            cutoff_resolves: true,
            cutoff_is_ancestor: true,
            index_tree: Some(INDEX_TREE),
            event_tree: Some(EVENT_TREE),
        },
        expected_predecessor: previous,
        expected_control_epoch: epoch,
    }
}

fn assert_invalid(
    value: &CompactionManifest,
    previous: Option<&str>,
    epoch: u64,
    expected: ValidationError,
) {
    assert_eq!(
        validate(value, &context(value, pack_bytes(), previous, epoch)),
        Err(expected)
    );
}

#[test]
fn complete_v2_manifest_round_trips_with_lossless_pack_index() {
    let value = manifest(Some(OLD_GENERATION), 4);
    assert!(value.generation_id.starts_with("v2-"));
    assert_eq!(
        value.archive_ref(),
        V2RefLayout::default().archive(&value.generation_id)
    );
    let bytes = serde_json::to_vec(&value).expect("serialize manifest");
    let parsed = parse_and_validate(
        &bytes,
        &context(&value, pack_bytes(), Some(OLD_GENERATION), 4),
    )
    .expect("complete manifest validates");
    let event = &parsed.packs[0].events[0];
    let raw = &pack_bytes()[event.offset as usize..(event.offset + event.bytes) as usize];
    assert_eq!(
        raw,
        pack_bytes(),
        "raw event bytes remain exactly recoverable"
    );
}

#[test]
fn canonical_paths_are_v2_only_and_legacy_paths_are_rejected() {
    let mut value = manifest(None, 0);
    value.snapshots.active.path = ".knots/snapshots/legacy.snapshot.json".to_string();
    value = value.seal();
    assert_eq!(
        validate(&value, &context(&value, pack_bytes(), None, 0)),
        Err(ValidationError::InvalidV2Path("active"))
    );
    let refs = V2RefLayout::default();
    assert_ne!(refs.legacy, refs.control);
    assert!(!refs
        .canonical("generation")
        .starts_with(&format!("{}/", refs.legacy)));
    assert_ne!(refs.archive("generation"), refs.inbox("writer-a"));
}

#[test]
fn pack_tampering_and_duplicate_event_ids_fail_validation() {
    let value = manifest(None, 0);
    assert_eq!(
        validate(&value, &context(&value, b"tampered", None, 0)),
        Err(ValidationError::LengthMismatch("pack"))
    );
    let mut duplicate = value.clone();
    duplicate.packs.push(duplicate.packs[0].clone());
    duplicate = duplicate.seal();
    assert_eq!(
        validate(&duplicate, &context(&duplicate, pack_bytes(), None, 0)),
        Err(ValidationError::DuplicatePack)
    );
}

#[test]
fn unknown_fields_and_incomplete_manifests_fail_closed() {
    let value = manifest(None, 0);
    let mut json = serde_json::to_value(&value).expect("manifest json");
    json["unexpected"] = serde_json::json!(true);
    assert!(matches!(
        parse_and_validate(
            &serde_json::to_vec(&json).unwrap(),
            &context(&value, pack_bytes(), None, 0)
        ),
        Err(ValidationError::InvalidJson(_))
    ));
}

fn marker(policy: &[u8]) -> ProtectionMarker {
    let refs = V2RefLayout::default();
    ProtectionMarker {
        schema_version: 1,
        repository_id: "repo-1".to_string(),
        policy_id: "ruleset-1".to_string(),
        policy_sha256: digest(policy),
        integrator_id: "actions-integrator".to_string(),
        control_ref: refs.control.to_string(),
        canonical_prefix: refs.canonical_prefix.to_string(),
        archive_prefix: refs.archive_prefix.to_string(),
        inbox_prefix: refs.inbox_prefix.to_string(),
    }
}

fn protection_facts<'a>(policy: &'a [u8]) -> ProviderProtectionFacts<'a> {
    let refs = V2RefLayout::default();
    ProviderProtectionFacts {
        repository_id: "repo-1",
        policy_id: "ruleset-1",
        policy_bytes: policy,
        integrator_id: "actions-integrator",
        control_ref: refs.control,
        canonical_prefix: refs.canonical_prefix,
        archive_prefix: refs.archive_prefix,
        inbox_prefix: refs.inbox_prefix,
        control_head: Some(CONTROL_HEAD),
        control_protected: true,
        canonical_create_only: true,
        archives_create_only: true,
        inboxes_writer_scoped: true,
    }
}

#[test]
fn protection_requires_matching_provider_facts_and_enforced_policy() {
    let policy = b"provider policy bytes";
    assert_eq!(
        validate_protection(None, None),
        Err(ProtectionError::Unavailable)
    );
    let marker = marker(policy);
    let mut facts = protection_facts(policy);
    facts.control_protected = false;
    assert_eq!(
        validate_protection(Some(&marker), Some(&facts)),
        Err(ProtectionError::PolicyNotEnforced)
    );
    let valid = validate_protection(Some(&marker), Some(&protection_facts(policy)))
        .expect("provider-backed marker validates");
    assert_eq!(valid.control_head(), Some(CONTROL_HEAD));

    let mut invalid = marker.clone();
    invalid.repository_id.clear();
    assert_eq!(
        validate_protection(Some(&invalid), Some(&protection_facts(policy))),
        Err(ProtectionError::InvalidMarker)
    );
    let mut mismatched = protection_facts(policy);
    mismatched.repository_id = "other-repo";
    assert_eq!(
        validate_protection(Some(&marker), Some(&mismatched)),
        Err(ProtectionError::ProviderMismatch)
    );
    assert!(ProtectionError::Unavailable
        .to_string()
        .contains("provider protection unavailable"));
}

#[test]
fn manifest_validation_rejects_each_authority_mismatch() {
    let predecessor = manifest(Some(OLD_GENERATION), 4);
    assert_invalid(&predecessor, None, 4, ValidationError::PredecessorMismatch);
    assert_invalid(
        &predecessor,
        Some(OLD_GENERATION),
        3,
        ValidationError::ControlEpochMismatch,
    );

    let mut value = manifest(None, 0);
    value.source.legacy_ref = "refs/heads/not-legacy".to_string();
    value = value.seal();
    assert_invalid(&value, None, 0, ValidationError::InvalidLegacyRef);

    let mut value = manifest(None, 0);
    value.writer_heads[0].writer_id.clear();
    value = value.seal();
    assert_invalid(&value, None, 0, ValidationError::InvalidWriterHead);

    let mut value = manifest(None, 0);
    value.writer_heads.push(value.writer_heads[0].clone());
    value = value.seal();
    assert_invalid(&value, None, 0, ValidationError::DuplicateWriter);

    let mut value = manifest(None, 0);
    value.packs[0].path = ".knots/v2/packs/wrong.pack".to_string();
    value = value.seal();
    assert_invalid(&value, None, 0, ValidationError::InvalidPackId);

    let mut value = manifest(None, 0);
    let duplicate_event = value.packs[0].events[0].clone();
    value.packs[0].events.push(duplicate_event);
    value = value.seal();
    assert_invalid(&value, None, 0, ValidationError::DuplicateEvent);

    let mut value = manifest(None, 0);
    value.packs[0].events[0].content_sha256 = digest(b"wrong event bytes");
    value = value.seal();
    assert_invalid(&value, None, 0, ValidationError::InvalidEventIndex);

    let mut value = manifest(None, 0);
    let wrong_digest = digest(b"wrong pack bytes");
    value.packs[0].sha256 = wrong_digest.clone();
    value.packs[0].pack_id = format!("pack-{wrong_digest}");
    value.packs[0].path = format!(".knots/v2/packs/pack-{wrong_digest}.pack");
    value = value.seal();
    assert_invalid(&value, None, 0, ValidationError::InvalidDigest("pack"));
    assert!(ValidationError::DuplicateEvent
        .to_string()
        .contains("invalid protocol-v2 manifest"));
}

fn control(
    epoch: u64,
    previous_head: Option<&str>,
    generation: &str,
    sequence: u64,
    action: ControlKind,
) -> ControlRecord {
    ControlRecord {
        schema_version: 1,
        epoch,
        previous_control_head: previous_head.map(str::to_string),
        active_generation_id: generation.to_string(),
        active_generation_commit: COMMIT.to_string(),
        archive_ref: V2RefLayout::default().archive(generation),
        acknowledged_writer_heads: vec![writer(sequence)],
        protection_policy_sha256: digest(b"provider policy bytes"),
        action,
    }
}

#[test]
fn activation_cas_is_monotonic_and_recovery_publishes_a_higher_epoch() {
    let initial = control(1, None, OLD_GENERATION, 5, ControlKind::Activation);
    let mut model = ProtocolModel::with_active(CONTROL_HEAD, initial);
    let next = manifest(Some(OLD_GENERATION), 1);
    model
        .prepare(&next)
        .expect("immutable generation is staged");
    let activation = control(
        2,
        Some(CONTROL_HEAD),
        &next.generation_id,
        7,
        ControlKind::Activation,
    );
    assert_eq!(
        model.activate(Some("stale"), NEXT_CONTROL_HEAD, activation.clone()),
        Err(ProtocolError::ActiveHeadMoved)
    );
    model
        .activate(Some(CONTROL_HEAD), NEXT_CONTROL_HEAD, activation)
        .expect("exact control-head CAS wins");

    let recovery_head = "6666666666666666666666666666666666666666";
    let recovery = control(
        3,
        Some(NEXT_CONTROL_HEAD),
        OLD_GENERATION,
        7,
        ControlKind::Recovery {
            rollback_of_epoch: 2,
        },
    );
    model
        .recover(NEXT_CONTROL_HEAD, recovery_head, recovery)
        .expect("rollback is a higher recovery epoch");
    assert_eq!(model.control_head(), Some(recovery_head));
    assert_eq!(model.active().map(|record| record.epoch), Some(3));
}

#[test]
fn recovery_cannot_regress_acknowledged_writer_heads() {
    let initial = control(1, None, OLD_GENERATION, 7, ControlKind::Activation);
    let mut model = ProtocolModel::with_active(CONTROL_HEAD, initial);
    let recovery = control(
        2,
        Some(CONTROL_HEAD),
        OLD_GENERATION,
        6,
        ControlKind::Recovery {
            rollback_of_epoch: 1,
        },
    );
    assert_eq!(
        model.recover(CONTROL_HEAD, NEXT_CONTROL_HEAD, recovery),
        Err(ProtocolError::WriterHeadRegressed)
    );
}

#[test]
fn control_record_must_bind_archive_commit_and_policy_digest() {
    let initial = control(1, None, OLD_GENERATION, 5, ControlKind::Activation);
    let mut model = ProtocolModel::with_active(CONTROL_HEAD, initial);
    let next = manifest(Some(OLD_GENERATION), 1);
    model.prepare(&next).expect("stage generation");
    let mut invalid = control(
        2,
        Some(CONTROL_HEAD),
        &next.generation_id,
        7,
        ControlKind::Activation,
    );
    invalid.archive_ref = "refs/heads/unprotected".to_string();
    assert_eq!(
        model.activate(Some(CONTROL_HEAD), NEXT_CONTROL_HEAD, invalid),
        Err(ProtocolError::InvalidControlRecord)
    );
}

#[test]
fn protocol_rejects_invalid_staging_activation_and_recovery_shapes() {
    let mut empty = ProtocolModel::default();
    let wrong_epoch = manifest(None, 1);
    assert_eq!(
        empty.prepare(&wrong_epoch),
        Err(ProtocolError::EpochNotMonotonic)
    );

    let prepared = manifest(None, 0);
    empty.prepare(&prepared).expect("stage first generation");
    let recovery = control(
        1,
        None,
        &prepared.generation_id,
        1,
        ControlKind::Recovery {
            rollback_of_epoch: 0,
        },
    );
    assert_eq!(
        empty.recover("", NEXT_CONTROL_HEAD, recovery),
        Err(ProtocolError::ActiveHeadMoved)
    );
    let recovery = control(
        1,
        None,
        &prepared.generation_id,
        1,
        ControlKind::Recovery {
            rollback_of_epoch: 0,
        },
    );
    assert_eq!(
        empty.recover(CONTROL_HEAD, NEXT_CONTROL_HEAD, recovery),
        Err(ProtocolError::ActiveHeadMoved)
    );

    let initial = control(1, None, OLD_GENERATION, 5, ControlKind::Activation);
    let mut active = ProtocolModel::with_active(CONTROL_HEAD, initial);
    let next = manifest(Some(OLD_GENERATION), 1);
    active.prepare(&next).expect("stage next generation");
    let invalid_activation = control(
        2,
        Some(CONTROL_HEAD),
        &next.generation_id,
        7,
        ControlKind::Recovery {
            rollback_of_epoch: 1,
        },
    );
    assert_eq!(
        active.activate(Some(CONTROL_HEAD), NEXT_CONTROL_HEAD, invalid_activation),
        Err(ProtocolError::InvalidRecovery)
    );

    let wrong_epoch = control(
        3,
        Some(CONTROL_HEAD),
        &next.generation_id,
        7,
        ControlKind::Activation,
    );
    assert_eq!(
        active.activate(Some(CONTROL_HEAD), NEXT_CONTROL_HEAD, wrong_epoch),
        Err(ProtocolError::EpochNotMonotonic)
    );

    let unknown = "v2-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let unknown_record = control(2, Some(CONTROL_HEAD), unknown, 7, ControlKind::Activation);
    assert_eq!(
        active.activate(Some(CONTROL_HEAD), NEXT_CONTROL_HEAD, unknown_record),
        Err(ProtocolError::UnknownGeneration)
    );
}
