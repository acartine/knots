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
