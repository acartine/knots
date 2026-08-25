use sha2::{Digest, Sha256};

use super::*;

const COMMIT: &str = "1111111111111111111111111111111111111111";
const INDEX_TREE: &str = "2222222222222222222222222222222222222222";
const EVENT_TREE: &str = "3333333333333333333333333333333333333333";
const OLD_GENERATION: &str = "g2-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn active_bytes() -> &'static [u8] {
    br#"{"schema_version":1,"written_at":"ignored","hot":[{}],"warm":[{}]}"#
}

fn cold_bytes() -> &'static [u8] {
    br#"{"schema_version":1,"written_at":"ignored","cold":[{}]}"#
}

fn descriptor(path: &str, bytes: &[u8], records: u64) -> SnapshotDescriptor {
    SnapshotDescriptor {
        path: path.to_string(),
        sha256: format!("{:x}", Sha256::digest(bytes)),
        bytes: bytes.len() as u64,
        records,
    }
}

fn manifest(state: GenerationState, previous_generation: Option<&str>) -> CompactionManifest {
    CompactionManifest {
        protocol_version: PROTOCOL_VERSION,
        generation_id: String::new(),
        state,
        source: SourceCheckpoint {
            remote_ref: "refs/work/knots".to_string(),
            cutoff_commit: COMMIT.to_string(),
            index_tree: INDEX_TREE.to_string(),
            event_tree: EVENT_TREE.to_string(),
        },
        snapshots: SnapshotSet {
            active: descriptor(
                ".knots/snapshots/checkpoint-active_catalog.snapshot.json",
                active_bytes(),
                2,
            ),
            cold: descriptor(
                ".knots/snapshots/checkpoint-cold_catalog.snapshot.json",
                cold_bytes(),
                1,
            ),
        },
        retention: Retention {
            max_full_files: 1_000,
            max_index_files: 1_000,
        },
        compatibility: Compatibility {
            minimum_reader_protocol: PROTOCOL_VERSION,
            minimum_writer_protocol: PROTOCOL_VERSION,
        },
        previous_generation: previous_generation.map(str::to_string),
    }
    .seal()
}

fn context<'a>(
    expected_predecessor: Option<&'a str>,
    predecessor_chain: &'a [&'a str],
) -> ValidationContext<'a> {
    ValidationContext {
        active_snapshot: Some(active_bytes()),
        cold_snapshot: Some(cold_bytes()),
        source: SourceFacts {
            cutoff_resolves: true,
            cutoff_is_ancestor: true,
            index_tree: Some(INDEX_TREE),
            event_tree: Some(EVENT_TREE),
        },
        expected_predecessor,
        predecessor_chain,
    }
}

#[test]
fn valid_manifest_round_trips_and_state_does_not_change_identity() {
    let prepared = manifest(GenerationState::Prepared, Some(OLD_GENERATION));
    assert_eq!(
        prepared.generation_id,
        "g2-9a1515ffc32b9543e49b7632b50f5105db39d382a536558d3ea9d978574f2f7d"
    );
    let mut active = prepared.clone();
    active.state = GenerationState::Active;
    assert_eq!(prepared.generation_id, active.expected_generation_id());

    let json = serde_json::to_vec(&active).expect("manifest should serialize");
    let parsed = parse_and_validate(&json, &context(Some(OLD_GENERATION), &[OLD_GENERATION]))
        .expect("complete manifest should validate");
    assert_eq!(parsed, active);
}

#[test]
fn parser_rejects_incomplete_unknown_and_unsupported_manifests() {
    let valid = manifest(GenerationState::Active, None);
    let mut value = serde_json::to_value(&valid).expect("manifest should serialize");
    value
        .as_object_mut()
        .expect("manifest should be an object")
        .remove("snapshots");
    assert!(matches!(
        parse_and_validate(&serde_json::to_vec(&value).unwrap(), &context(None, &[])),
        Err(ValidationError::InvalidJson(_))
    ));

    let mut value = serde_json::to_value(&valid).unwrap();
    value["unexpected"] = serde_json::json!(true);
    assert!(matches!(
        parse_and_validate(&serde_json::to_vec(&value).unwrap(), &context(None, &[])),
        Err(ValidationError::InvalidJson(_))
    ));

    let unsupported = CompactionManifest {
        protocol_version: 3,
        ..valid
    }
    .seal();
    assert_eq!(
        validate(&unsupported, &context(None, &[])),
        Err(ValidationError::UnsupportedProtocol(3))
    );
}

#[test]
fn validation_rejects_identity_compatibility_ref_and_retention_errors() {
    let mut value = manifest(GenerationState::Active, None);
    value.generation_id.push('0');
    assert_eq!(
        validate(&value, &context(None, &[])),
        Err(ValidationError::InvalidGenerationId)
    );

    let mut value = manifest(GenerationState::Active, None);
    value.compatibility.minimum_writer_protocol = 3;
    value = value.seal();
    assert_eq!(
        validate(&value, &context(None, &[])),
        Err(ValidationError::UnsupportedCompatibility)
    );

    let mut value = manifest(GenerationState::Active, None);
    value.source.remote_ref = "knots-v2".to_string();
    value = value.seal();
    assert_eq!(
        validate(&value, &context(None, &[])),
        Err(ValidationError::InvalidRemoteRef)
    );

    for remote_ref in [
        "refs/work/bad ref",
        "refs/work/topic.lock",
        "refs/work/@{bad}",
        "refs//work",
    ] {
        let mut value = manifest(GenerationState::Active, None);
        value.source.remote_ref = remote_ref.to_string();
        value = value.seal();
        assert_eq!(
            validate(&value, &context(None, &[])),
            Err(ValidationError::InvalidRemoteRef)
        );
    }

    let mut value = manifest(GenerationState::Active, None);
    value.retention.max_full_files = 0;
    value = value.seal();
    assert_eq!(
        validate(&value, &context(None, &[])),
        Err(ValidationError::InvalidRetention)
    );
}

#[test]
fn validation_rejects_unresolved_or_inconsistent_cutoff_facts() {
    let value = manifest(GenerationState::Active, None);
    let mut bad_id = value.clone();
    bad_id.source.cutoff_commit = "not-an-object".to_string();
    bad_id = bad_id.seal();
    assert_eq!(
        validate(&bad_id, &context(None, &[])),
        Err(ValidationError::InvalidObjectId("cutoff_commit"))
    );

    let mut unresolved = context(None, &[]);
    unresolved.source.cutoff_resolves = false;
    assert_eq!(
        validate(&value, &unresolved),
        Err(ValidationError::UnresolvedSource)
    );

    let mut wrong_tree = context(None, &[]);
    wrong_tree.source.index_tree = Some(EVENT_TREE);
    assert_eq!(
        validate(&value, &wrong_tree),
        Err(ValidationError::TreeMismatch("index_tree"))
    );

    let mut unrelated = context(None, &[]);
    unrelated.source.cutoff_is_ancestor = false;
    assert_eq!(
        validate(&value, &unrelated),
        Err(ValidationError::CutoffNotAncestor)
    );
}

#[test]
fn validation_rejects_missing_corrupt_and_miscounted_snapshots() {
    let value = manifest(GenerationState::Active, None);
    let mut missing = context(None, &[]);
    missing.active_snapshot = None;
    assert_eq!(
        validate(&value, &missing),
        Err(ValidationError::MissingSnapshot("active"))
    );

    let mut corrupt = context(None, &[]);
    corrupt.cold_snapshot = Some(b"different");
    assert_eq!(
        validate(&value, &corrupt),
        Err(ValidationError::SnapshotLengthMismatch("cold"))
    );

    let mut bad_path = value.clone();
    bad_path.snapshots.active.path = "../checkpoint.snapshot.json".to_string();
    bad_path = bad_path.seal();
    assert_eq!(
        validate(&bad_path, &context(None, &[])),
        Err(ValidationError::InvalidSnapshotPath("active"))
    );

    let mut bad_count = value;
    bad_count.snapshots.cold.records = 2;
    bad_count = bad_count.seal();
    assert_eq!(
        validate(&bad_count, &context(None, &[])),
        Err(ValidationError::SnapshotCountMismatch("cold"))
    );
}

#[test]
fn validation_rejects_snapshot_digest_json_and_schema_errors() {
    let mut malformed_digest = manifest(GenerationState::Active, None);
    malformed_digest.snapshots.active.sha256 = "ABC".to_string();
    malformed_digest = malformed_digest.seal();
    assert_eq!(
        validate(&malformed_digest, &context(None, &[])),
        Err(ValidationError::InvalidSnapshotDigest("active"))
    );

    let mut different = active_bytes().to_vec();
    different[1] = b'X';
    let mut wrong_digest = context(None, &[]);
    wrong_digest.active_snapshot = Some(&different);
    assert_eq!(
        validate(&manifest(GenerationState::Active, None), &wrong_digest),
        Err(ValidationError::InvalidSnapshotDigest("active"))
    );

    let invalid_json = b"not-json";
    let mut invalid = manifest(GenerationState::Active, None);
    invalid.snapshots.active = descriptor(
        ".knots/snapshots/checkpoint-active_catalog.snapshot.json",
        invalid_json,
        0,
    );
    invalid = invalid.seal();
    let mut invalid_context = context(None, &[]);
    invalid_context.active_snapshot = Some(invalid_json);
    assert_eq!(
        validate(&invalid, &invalid_context),
        Err(ValidationError::InvalidSnapshotJson("active"))
    );

    let wrong_schema = br#"{"schema_version":2,"hot":[],"warm":[]}"#;
    let mut unsupported = manifest(GenerationState::Active, None);
    unsupported.snapshots.active = descriptor(
        ".knots/snapshots/checkpoint-active_catalog.snapshot.json",
        wrong_schema,
        0,
    );
    unsupported = unsupported.seal();
    let mut unsupported_context = context(None, &[]);
    unsupported_context.active_snapshot = Some(wrong_schema);
    assert_eq!(
        validate(&unsupported, &unsupported_context),
        Err(ValidationError::SnapshotSchemaMismatch("active"))
    );
}

#[test]
fn validation_rejects_predecessor_skips_and_cycles() {
    let value = manifest(GenerationState::Active, Some(OLD_GENERATION));
    assert_eq!(
        validate(&value, &context(None, &[])),
        Err(ValidationError::PredecessorMismatch)
    );
    assert_eq!(
        validate(
            &value,
            &context(Some(OLD_GENERATION), &[OLD_GENERATION, OLD_GENERATION]),
        ),
        Err(ValidationError::PredecessorCycle)
    );
    assert_eq!(
        validate(
            &value,
            &context(Some(OLD_GENERATION), &[value.generation_id.as_str()]),
        ),
        Err(ValidationError::PredecessorCycle)
    );
}

#[test]
fn interrupted_prepare_never_changes_the_active_generation() {
    let previous = OLD_GENERATION;
    let prepared = manifest(GenerationState::Prepared, Some(previous));
    let mut model = ProtocolModel::with_active(previous);
    model.prepare(&prepared).expect("prepare should stage");
    assert_eq!(
        model.prepared_generation(),
        Some(prepared.generation_id.as_str())
    );

    model.interrupt_prepare();
    assert_eq!(model.active_generation(), Some(previous));
    assert_eq!(model.prepared_generation(), None);
    let mut active = prepared;
    active.state = GenerationState::Active;
    assert_eq!(
        model.activate(Some(previous), &active),
        Err(ProtocolError::NoPreparedGeneration)
    );
}

#[test]
fn activation_is_compare_and_swap_and_rollback_uses_verified_history() {
    let previous = OLD_GENERATION;
    let prepared = manifest(GenerationState::Prepared, Some(previous));
    let mut active = prepared.clone();
    active.state = GenerationState::Active;
    let mut model = ProtocolModel::with_active(previous);
    model.prepare(&prepared).unwrap();
    assert_eq!(
        model.activate(Some("g2-lost-race"), &active),
        Err(ProtocolError::ActiveHeadMoved)
    );
    model.activate(Some(previous), &active).unwrap();
    assert_eq!(
        model.active_generation(),
        Some(active.generation_id.as_str())
    );

    assert_eq!(
        model.rollback(active.generation_id.as_str(), "g2-unverified"),
        Err(ProtocolError::UnknownRollbackTarget)
    );
    model
        .rollback(active.generation_id.as_str(), previous)
        .expect("verified predecessor should be restorable");
    assert_eq!(model.active_generation(), Some(previous));
}

#[test]
fn protocol_rejects_wrong_states_duplicate_staging_and_mismatched_activation() {
    let previous = OLD_GENERATION;
    let prepared = manifest(GenerationState::Prepared, Some(previous));
    let mut active = prepared.clone();
    active.state = GenerationState::Active;
    let mut model = ProtocolModel::with_active(previous);
    assert_eq!(model.prepare(&active), Err(ProtocolError::WrongState));
    model.prepare(&prepared).unwrap();
    assert_eq!(
        model.prepare(&prepared),
        Err(ProtocolError::GenerationAlreadyStaged)
    );

    let mut other = manifest(GenerationState::Active, Some(previous));
    other.retention.max_full_files = 999;
    other = other.seal();
    assert_eq!(
        model.activate(Some(previous), &other),
        Err(ProtocolError::PreparedGenerationMismatch)
    );
    assert_eq!(
        model.activate(Some(previous), &prepared),
        Err(ProtocolError::WrongState)
    );
}
