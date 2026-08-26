use std::collections::HashSet;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::*;
use crate::db::{self, ColdCatalogRecord, KnotCacheRecord, WarmKnotRecord};

const CONTROL_HEAD: &str = "1111111111111111111111111111111111111111";
const GENERATION_COMMIT: &str = "2222222222222222222222222222222222222222";
const CUTOFF: &str = "3333333333333333333333333333333333333333";
const INDEX_TREE: &str = "4444444444444444444444444444444444444444";
const EVENT_TREE: &str = "5555555555555555555555555555555555555555";
const WRITER_COMMIT: &str = "6666666666666666666666666666666666666666";
pub(super) struct Fixture {
    control: Vec<u8>,
    manifest: Vec<u8>,
    active: Vec<u8>,
    cold: Vec<u8>,
    projections: Vec<u8>,
    pack: BuiltPack,
    protection: ValidatedProtection,
}
struct CatalogArtifacts {
    active: Vec<u8>,
    cold: Vec<u8>,
    projections: Vec<u8>,
    pack: BuiltPack,
}
impl Fixture {
    pub(super) fn input(&self) -> CheckpointInput<'_> {
        CheckpointInput {
            control_head: CONTROL_HEAD,
            control: &self.control,
            manifest: &self.manifest,
            active_snapshot: &self.active,
            cold_snapshot: &self.cold,
            projections: &self.projections,
            packs: vec![(self.pack.descriptor.pack_id.as_str(), &self.pack.compressed)],
            source: SourceFacts {
                cutoff_resolves: true,
                cutoff_is_ancestor: true,
                index_tree: Some(INDEX_TREE),
                event_tree: Some(EVENT_TREE),
            },
            expected_predecessor: None,
            expected_previous_control_head: None,
            expected_control_epoch: 0,
            protection: &self.protection,
        }
    }
}
pub(super) fn fixture() -> Fixture {
    let artifacts = catalog_artifacts();
    let writer = writer_head();
    let manifest = manifest(&artifacts, &writer);
    let policy = b"protected-policy";
    let control = ControlRecord {
        schema_version: 1,
        epoch: 1,
        previous_control_head: None,
        active_generation_id: manifest.generation_id.clone(),
        active_generation_commit: GENERATION_COMMIT.to_string(),
        archive_ref: manifest.archive_ref(),
        acknowledged_writer_heads: vec![writer],
        protection_policy_sha256: digest(policy),
        action: ControlKind::Activation,
    };
    Fixture {
        control: serde_json::to_vec(&control).unwrap(),
        manifest: serde_json::to_vec(&manifest).unwrap(),
        active: artifacts.active,
        cold: artifacts.cold,
        projections: artifacts.projections,
        pack: artifacts.pack,
        protection: protection(policy),
    }
}

fn catalog_artifacts() -> CatalogArtifacts {
    let hot = hot_record("hot", "Hot");
    let warm = WarmKnotRecord {
        id: "warm".to_string(),
        title: "Warm".to_string(),
    };
    let cold = ColdCatalogRecord {
        id: "cold".to_string(),
        title: "Cold".to_string(),
        state: "shipped".to_string(),
        updated_at: "2026-08-25T20:00:00Z".to_string(),
    };
    let active = serde_json::to_vec(&json!({
        "schema_version": 1,
        "hot": [&hot],
        "warm": [&warm],
    }))
    .unwrap();
    let cold_bytes = serde_json::to_vec(&json!({
        "schema_version": 1,
        "cold": [&cold],
    }))
    .unwrap();
    let projections = CanonicalProjections {
        schema_version: 1,
        hot: vec![serde_json::to_value(&hot).unwrap()],
        warm: vec![serde_json::to_value(&warm).unwrap()],
        cold: vec![serde_json::to_value(&cold).unwrap()],
        edges: vec![json!({"src": "hot", "kind": "blocked_by", "dst": "cold"})],
        leases: vec![json!({"id": "lease-1"})],
        workflows: vec![json!({"id": "work_sdlc"})],
        metadata: vec![json!({"id": "note-1"})],
        conflicts: vec![],
    };
    let projection_bytes = canonical_projection_bytes(&projections).unwrap();
    let pack = fixture_pack();
    CatalogArtifacts {
        active,
        cold: cold_bytes,
        projections: projection_bytes,
        pack,
    }
}

fn writer_head() -> WriterHead {
    WriterHead {
        writer_id: "writer-a".to_string(),
        inbox_ref: V2RefLayout::default().inbox("writer-a"),
        commit: WRITER_COMMIT.to_string(),
        sequence: 1,
    }
}

fn manifest(artifacts: &CatalogArtifacts, writer: &WriterHead) -> CompactionManifest {
    CompactionManifest {
        protocol_version: PROTOCOL_VERSION,
        generation_id: String::new(),
        predecessor_generation: None,
        predecessor_control_epoch: 0,
        source: SourceCheckpoint {
            legacy_ref: LEGACY_REF.to_string(),
            cutoff_commit: CUTOFF.to_string(),
            index_tree: INDEX_TREE.to_string(),
            event_tree: EVENT_TREE.to_string(),
        },
        writer_heads: vec![writer.clone()],
        snapshots: SnapshotSet {
            active: descriptor(
                ".knots/v2/generations/current/active.snapshot.json",
                &artifacts.active,
                2,
            ),
            cold: descriptor(
                ".knots/v2/generations/current/cold.snapshot.json",
                &artifacts.cold,
                1,
            ),
        },
        projections: projection_descriptor(&artifacts.projections, 7),
        packs: vec![artifacts.pack.descriptor.clone()],
        compatibility: Compatibility {
            minimum_reader_protocol: PROTOCOL_VERSION,
            minimum_writer_protocol: PROTOCOL_VERSION,
        },
    }
    .seal()
}

fn protection(policy: &[u8]) -> ValidatedProtection {
    let policy_sha256 = digest(policy);
    let layout = V2RefLayout::default();
    let marker = ProtectionMarker {
        schema_version: 1,
        repository_id: "repo-1".to_string(),
        policy_id: "policy-1".to_string(),
        policy_sha256,
        integrator_id: "integrator-1".to_string(),
        control_ref: layout.control.to_string(),
        canonical_prefix: layout.canonical_prefix.to_string(),
        archive_prefix: layout.archive_prefix.to_string(),
        inbox_prefix: layout.inbox_prefix.to_string(),
    };
    let facts = ProviderProtectionFacts {
        repository_id: "repo-1",
        policy_id: "policy-1",
        policy_bytes: policy,
        integrator_id: "integrator-1",
        control_ref: layout.control,
        canonical_prefix: layout.canonical_prefix,
        archive_prefix: layout.archive_prefix,
        inbox_prefix: layout.inbox_prefix,
        control_head: Some(CONTROL_HEAD),
        control_protected: true,
        canonical_create_only: true,
        archives_create_only: true,
        inboxes_writer_scoped: true,
    };
    validate_protection(Some(&marker), Some(&facts)).unwrap()
}

#[test]
fn fresh_bootstrap_replaces_every_projection_and_records_exact_generation() {
    let conn = db::open_connection(":memory:").unwrap();
    let identity = db::ensure_writer_identity(&conn).unwrap();
    seed_old_cache(&conn);
    db::insert_edge(&conn, "old", "relates_to", "local-target").unwrap();
    db::quarantine_knot_if_absent(&conn, "held-old", CUTOFF).unwrap();
    conn.execute(
        "INSERT INTO v2_legacy_quarantine VALUES ('legacy.json', 'hash', 'now')",
        [],
    )
    .unwrap();
    let fixture = fixture();
    let prepared = prepare_checkpoint(fixture.input()).unwrap();
    let summary = install_checkpoint(
        &conn,
        &prepared,
        &HashSet::from(["old".to_string()]),
        |tx| {
            tx.execute(
                "INSERT INTO knot_warm(id, title) VALUES ('delta', 'Retained delta')",
                [],
            )
            .map_err(|error| error.to_string())?;
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(
        (summary.hot, summary.warm, summary.cold, summary.edges),
        (1, 1, 1, 1)
    );
    assert_eq!(summary.quarantined, 2);
    assert_eq!(
        db::get_knot_hot(&conn, "old").unwrap().unwrap().title,
        "Old"
    );
    assert!(db::get_knot_hot(&conn, "hot").unwrap().is_some());
    assert_eq!(db::list_knot_warm(&conn).unwrap().len(), 2);
    assert_eq!(
        db::list_edges(&conn, "hot", db::EdgeDirection::Outgoing)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        db::list_edges(&conn, "old", db::EdgeDirection::Outgoing)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        scalar(&conn, "SELECT COUNT(*) FROM v2_checkpoint_writer_head"),
        1
    );
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM v2_checkpoint_event"), 1);
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM v2_projection_aux"), 3);
    assert_eq!(
        scalar(&conn, "SELECT COUNT(*) FROM v2_generation_quarantine"),
        2
    );
    assert_eq!(
        scalar(&conn, "SELECT COUNT(*) FROM knot_sync_quarantine"),
        0
    );
    assert_eq!(
        scalar(&conn, "SELECT COUNT(*) FROM v2_legacy_quarantine"),
        1
    );
    assert_eq!(db::ensure_writer_identity(&conn).unwrap(), identity);
    assert_eq!(
        db::get_meta(&conn, "last_index_head_commit")
            .unwrap()
            .as_deref(),
        Some(CUTOFF)
    );

    assert!(drain_generation_quarantine(&conn, "old", |tx| {
        db::delete_knot_hot(tx, "old").map_err(|error| error.to_string())?;
        db::upsert_knot_warm(tx, "old", "Released from generation")
            .map_err(|error| error.to_string())?;
        Ok(())
    })
    .unwrap());
    assert!(db::get_knot_hot(&conn, "old").unwrap().is_none());
    assert_eq!(
        db::get_knot_warm(&conn, "old").unwrap().unwrap().title,
        "Released from generation"
    );
    assert_eq!(
        scalar(&conn, "SELECT COUNT(*) FROM v2_generation_quarantine"),
        1
    );
}

#[test]
fn sync_service_owns_the_authenticated_checkpoint_boundary() {
    let conn = db::open_connection(":memory:").unwrap();
    let ws = knots_test_support::workspace("checkpoint-sync-boundary");
    let service = crate::sync::SyncService::new(&conn, ws.path().to_path_buf());
    let fixture = fixture();
    let manifest: CompactionManifest = serde_json::from_slice(&fixture.manifest).unwrap();
    let summary = service
        .install_v2_checkpoint(fixture.input(), |_| Ok(()))
        .unwrap();
    assert_eq!(summary.generation_id, manifest.generation_id);
    assert!(!service.drain_v2_quarantine("absent", |_| Ok(())).unwrap());
}

#[test]
fn replay_failure_rolls_back_projection_watermark_and_quarantine_replacement() {
    let conn = db::open_connection(":memory:").unwrap();
    seed_old_cache(&conn);
    db::set_meta(&conn, "last_index_head_commit", "old-head").unwrap();
    db::quarantine_knot_if_absent(&conn, "held-old", "old-head").unwrap();
    let fixture = fixture();
    let prepared = prepare_checkpoint(fixture.input()).unwrap();
    let result = install_checkpoint(&conn, &prepared, &HashSet::new(), |tx| {
        tx.execute("INSERT INTO knot_warm VALUES ('partial', 'Partial')", [])
            .map_err(|error| error.to_string())?;
        Err("simulated crash".to_string())
    });
    assert!(matches!(result, Err(CheckpointError::Replay(_))));
    assert!(db::get_knot_hot(&conn, "old").unwrap().is_some());
    assert!(db::get_knot_hot(&conn, "hot").unwrap().is_none());
    assert_eq!(
        db::get_meta(&conn, "last_index_head_commit")
            .unwrap()
            .as_deref(),
        Some("old-head")
    );
    assert_eq!(
        scalar(&conn, "SELECT COUNT(*) FROM knot_sync_quarantine"),
        1
    );
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM v2_checkpoint"), 0);
}

#[test]
fn corrupt_or_projection_incomplete_generation_is_rejected_before_sqlite_changes() {
    let fixture = fixture();
    let mut corrupt = fixture.projections.clone();
    corrupt.push(b' ');
    let mut input = fixture.input();
    input.projections = &corrupt;
    assert!(matches!(
        prepare_checkpoint(input),
        Err(CheckpointError::Validation(_))
    ));

    let mut mismatch_value: Value = serde_json::from_slice(&fixture.projections).unwrap();
    mismatch_value["hot"] = json!([]);
    let mismatch = serde_json::to_vec(&mismatch_value).unwrap();
    let mut manifest: CompactionManifest = serde_json::from_slice(&fixture.manifest).unwrap();
    manifest.projections = projection_descriptor(&mismatch, manifest.projections.records - 1);
    manifest = manifest.seal();
    let mut control: ControlRecord = serde_json::from_slice(&fixture.control).unwrap();
    control.active_generation_id = manifest.generation_id.clone();
    control.archive_ref = manifest.archive_ref();
    let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
    let control_bytes = serde_json::to_vec(&control).unwrap();
    let mut mismatch_input = fixture.input();
    mismatch_input.manifest = &manifest_bytes;
    mismatch_input.control = &control_bytes;
    mismatch_input.projections = &mismatch;
    assert!(matches!(
        prepare_checkpoint(mismatch_input),
        Err(CheckpointError::Projection(_))
    ));
}

#[test]
fn schema_upgrade_is_atomic_and_checkpoint_tables_are_present() {
    let ws = knots_test_support::workspace("checkpoint-migration");
    let path = ws.path().join("state.sqlite");
    let conn = db::open_connection(path.to_str().unwrap()).unwrap();
    let schema_version = db::CURRENT_SCHEMA_VERSION.to_string();
    assert_eq!(
        db::get_meta(&conn, "schema_version").unwrap().as_deref(),
        Some(schema_version.as_str())
    );
    assert_eq!(
        scalar(
            &conn,
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 22"
        ),
        1
    );
    assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM v2_checkpoint"), 0);
}

fn hot_record(id: &str, title: &str) -> KnotCacheRecord {
    serde_json::from_value(json!({
        "id": id,
        "title": title,
        "state": "ready_for_implementation",
        "updated_at": "2026-08-25T20:00:00Z",
        "body": null,
        "description": null,
        "acceptance": null,
        "priority": null,
        "knot_type": "work",
        "tags": [],
        "notes": [],
        "handoff_capsules": [],
        "invariants": [],
        "verification_steps": [],
        "step_history": [],
        "gate_data": {},
        "lease_data": {},
        "execution_plan_data": {},
        "scope_data": {},
        "lease_id": null,
        "lease_expiry_ts": 0,
        "workflow_id": "work_sdlc",
        "profile_id": "autopilot",
        "profile_etag": null,
        "deferred_from_state": null,
        "blocked_from_state": null,
        "created_at": "2026-08-25T20:00:00Z"
    }))
    .unwrap()
}

fn seed_old_cache(conn: &rusqlite::Connection) {
    let record = hot_record("old", "Old");
    let value = serde_json::to_value(record).unwrap();
    let columns = [
        "id",
        "title",
        "state",
        "updated_at",
        "tags_json",
        "notes_json",
        "handoff_capsules_json",
        "invariants_json",
        "verification_steps_json",
        "step_history_json",
        "gate_data_json",
        "lease_data_json",
        "execution_plan_data_json",
        "scope_data_json",
        "workflow_id",
        "profile_id",
    ];
    let sql = format!(
        "INSERT INTO knot_hot ({}) VALUES (?1,?2,?3,?4,'[]','[]','[]','[]','[]','[]','{{}}','{{}}','{{}}','{{}}',?5,?6)",
        columns.join(",")
    );
    conn.execute(
        &sql,
        rusqlite::params![
            value["id"].as_str().unwrap(),
            value["title"].as_str().unwrap(),
            value["state"].as_str().unwrap(),
            value["updated_at"].as_str().unwrap(),
            value["workflow_id"].as_str().unwrap(),
            value["profile_id"].as_str().unwrap(),
        ],
    )
    .unwrap();
}

fn descriptor(path: &str, bytes: &[u8], records: u64) -> SnapshotDescriptor {
    SnapshotDescriptor {
        path: path.to_string(),
        sha256: digest(bytes),
        bytes: bytes.len() as u64,
        records,
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn scalar(conn: &rusqlite::Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |row| row.get(0)).unwrap()
}
