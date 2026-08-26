pub(super) const REQUIRED_META_DEFAULT_KEYS: [&str; 7] = [
    "hot_window_days",
    "sync_policy",
    "sync_auto_budget_ms",
    "sync_try_lock_ms",
    "push_retry_budget_ms",
    "sync_fetch_blob_limit_kb",
    "pull_drift_warn_threshold",
];

pub(super) const PROTOCOL_V2_OUTBOX: &str = r#"
CREATE TABLE IF NOT EXISTS v2_writer_epoch (
    writer_id TEXT PRIMARY KEY,
    credential_id TEXT NOT NULL,
    inbox_ref TEXT NOT NULL UNIQUE,
    expected_inbox_oid TEXT,
    next_sequence INTEGER NOT NULL DEFAULT 1,
    registered INTEGER NOT NULL DEFAULT 0,
    active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_v2_writer_epoch_active
ON v2_writer_epoch(active) WHERE active = 1;

CREATE TABLE IF NOT EXISTS v2_outbox (
    event_id TEXT PRIMARY KEY,
    stream TEXT NOT NULL,
    relative_path TEXT NOT NULL UNIQUE,
    content_sha256 TEXT NOT NULL,
    payload BLOB NOT NULL,
    writer_id TEXT,
    sequence INTEGER,
    proposed_inbox_oid TEXT,
    acknowledged_inbox_oid TEXT,
    created_at TEXT NOT NULL,
    acknowledged_at TEXT,
    FOREIGN KEY(writer_id) REFERENCES v2_writer_epoch(writer_id),
    UNIQUE(writer_id, sequence)
);

CREATE INDEX IF NOT EXISTS idx_v2_outbox_pending
ON v2_outbox(acknowledged_at, created_at, event_id);

CREATE TABLE IF NOT EXISTS v2_legacy_quarantine (
    relative_path TEXT PRIMARY KEY,
    content_sha256 TEXT NOT NULL,
    inventoried_at TEXT NOT NULL
);
"#;

pub(super) const PROTOCOL_V2_CHECKPOINT: &str = r#"
CREATE TABLE IF NOT EXISTS v2_checkpoint (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    generation_id TEXT NOT NULL,
    control_epoch INTEGER NOT NULL,
    control_head TEXT NOT NULL,
    generation_commit TEXT NOT NULL,
    cutoff_commit TEXT NOT NULL,
    applied_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS v2_checkpoint_writer_head (
    writer_id TEXT PRIMARY KEY,
    inbox_ref TEXT NOT NULL,
    commit_oid TEXT NOT NULL,
    sequence INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS v2_checkpoint_event (
    event_id TEXT PRIMARY KEY,
    content_sha256 TEXT NOT NULL,
    pack_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS v2_projection_aux (
    kind TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    payload_json TEXT NOT NULL,
    PRIMARY KEY (kind, ordinal)
);

CREATE TABLE IF NOT EXISTS v2_generation_quarantine (
    knot_id TEXT PRIMARY KEY,
    base_generation_id TEXT NOT NULL,
    writer_vector_json TEXT NOT NULL,
    quarantined_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS v2_writer_identity (
    writer_id TEXT PRIMARY KEY,
    algorithm TEXT NOT NULL,
    public_key BLOB NOT NULL UNIQUE,
    public_key_sha256 TEXT NOT NULL UNIQUE,
    private_seed BLOB NOT NULL,
    generation INTEGER NOT NULL UNIQUE,
    parent_writer_id TEXT,
    active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    retired_at TEXT,
    FOREIGN KEY(parent_writer_id) REFERENCES v2_writer_identity(writer_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_v2_writer_identity_active
ON v2_writer_identity(active) WHERE active = 1;
"#;

pub(super) const PROTOCOL_V2_SIGNED_BATCHES: &str = r#"
ALTER TABLE v2_outbox RENAME TO v2_outbox_pre_signed_batches;

CREATE TABLE v2_outbox (
    event_id TEXT PRIMARY KEY,
    stream TEXT NOT NULL,
    relative_path TEXT NOT NULL UNIQUE,
    content_sha256 TEXT NOT NULL,
    payload BLOB NOT NULL,
    writer_id TEXT,
    sequence INTEGER,
    proposed_inbox_oid TEXT,
    proposal_base_generation TEXT,
    acknowledged_inbox_oid TEXT,
    created_at TEXT NOT NULL,
    acknowledged_at TEXT,
    FOREIGN KEY(writer_id) REFERENCES v2_writer_epoch(writer_id)
);

INSERT INTO v2_outbox (
    event_id, stream, relative_path, content_sha256, payload, writer_id, sequence,
    proposed_inbox_oid, proposal_base_generation, acknowledged_inbox_oid,
    created_at, acknowledged_at
)
SELECT event_id, stream, relative_path, content_sha256, payload, writer_id, sequence,
       proposed_inbox_oid, NULL, acknowledged_inbox_oid, created_at, acknowledged_at
FROM v2_outbox_pre_signed_batches;

DROP TABLE v2_outbox_pre_signed_batches;

CREATE INDEX idx_v2_outbox_pending
ON v2_outbox(acknowledged_at, created_at, event_id);

UPDATE v2_outbox
SET writer_id = NULL, sequence = NULL, proposed_inbox_oid = NULL
WHERE acknowledged_at IS NULL AND proposed_inbox_oid IS NULL;

UPDATE v2_writer_epoch
SET next_sequence = COALESCE((
    SELECT MAX(sequence) + 1 FROM v2_outbox
    WHERE v2_outbox.writer_id = v2_writer_epoch.writer_id
), 1);
"#;
