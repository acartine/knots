# db

SQLite cache layer. Events are the source of truth; this is a materialized view.

## Key Files

- **`mod.rs`** — `open_or_create()`, `upsert_knot_warm()`, `query_knots()`
- **`migrations.rs`** — schema version 13, migration pipeline
- **`catalog.rs`** — warm/cold catalog ops, edge queries, config helpers
- **`tests.rs`** — unit tests for core queries

## Key Types

- `Connection` (rusqlite) — all queries go through this
- Schema uses WAL mode, 5s busy timeout, foreign keys enabled
