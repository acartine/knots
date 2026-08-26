# db

SQLite cache layer. Events are the source of truth; this is a materialized view.

## Key Files

- **`../db.rs`** — module root: `open_connection()`, `upsert_knot_hot()`, `list_knot_hot()`, `CURRENT_SCHEMA_VERSION`
- **`writer_identity.rs`** — stores Ed25519 seeds and public metadata transactionally in the
  local-only, mode-0600 cache database. Fingerprints are ref-safe writer ids. Rotation retains
  prior keys and lineage so acknowledged writer history remains auditable.
- **`migrations.rs`** — 17 sequential migrations (current schema version 17)
- **`catalog.rs`** — warm/cold catalog ops, edge queries, config helpers
- **`tests.rs`** / **`tests_ext.rs`** — query and migration tests

## Key Types

- `Connection` (rusqlite) — all queries go through this
- `KnotCacheRecord`, `WarmKnotRecord`, `ColdCatalogRecord`, `EdgeRecord`
- Schema uses WAL mode, 5s busy timeout, foreign keys enabled
