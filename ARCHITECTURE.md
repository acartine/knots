# Knots Architecture

Local-first, event-sourced workflow tracker backed by git and SQLite.

## Data/Control Flow

```
CLI (cli.rs, clap)
 ├─ Write path: write_dispatch → write_queue (FIFO) → App methods → EventWriter + SQLite
 └─ Read path:  run_commands  → App::open_with_context() → SQLite queries
```

Events are the source of truth; SQLite is a materialized cache.
`kno push/pull` replicates events via a git worktree on the `knots` branch.

## Directory Map

| Path | Purpose |
|------|---------|
| [`src/app/`](src/app/README.md) | Core business logic: knot CRUD, state transitions, gates, edges |
| [`src/domain/`](src/domain/README.md) | Value types: KnotType, GateData, LeaseData, Invariant, MetadataEntry |
| [`src/db/`](src/db/README.md) | SQLite cache: schema, migrations, warm/cold queries |
| [`src/events/`](src/events/README.md) | Event file I/O: write JSON events to `.knots/events/` and `.knots/index/` |
| [`src/sync/`](src/sync/README.md) | Git replication: pull, push, incremental event application |
| [`src/installed_workflows/`](src/installed_workflows/README.md) | Workflow/profile loading, TOML/JSON parsing, validation |
| [`src/write_dispatch/`](src/write_dispatch/README.md) | Write command routing and execution via queued operations |
| [`src/poll_claim/`](src/poll_claim/README.md) | Poll/claim: find and claim highest-priority ready knots |
| [`src/ui/`](src/ui/README.md) | Terminal output: colored knot display, doctor reports, progress |
| [`tests/`](tests/README.md) | Integration tests: CLI dispatch, workflows, hierarchy, skills |
| [`scripts/`](scripts/README.md) | Build/release scripts and git hooks |

## Key Entry Points

- **Binary**: `main.rs` → `run()` — CLI bootstrap and command dispatch
- **Write ops**: `write_dispatch/operation_map.rs` → `operation_from_command()` — maps CLI to operations
- **Read ops**: `run_commands.rs` → `dispatch_read_command()` — handles ls, show, pull, push, etc.
- **App layer**: `app.rs` → `App` struct — opens SQLite + event writer, delegates to submodules
- **Sync**: `sync/mod.rs` → `pull()`, `push()` — git-based event replication

## Build System

Makefile targets (`make sanity` runs all before push):
- `fmt`: `cargo fmt --all -- --check`
- `lint`: `cargo clippy` + `scripts/repo/check-file-sizes.sh`
- `test`: `cargo test --all-targets --all-features`
- `coverage`: `cargo tarpaulin` (threshold in `.ci/coverage-threshold.txt`)

## Key Invariants

- Events are append-only and immutable once written
- Write operations serialize through `write_queue` (file-lock + FIFO)
- Lock ordering: repo.lock → cache.lock (never reverse)
- SQLite uses WAL mode with 5s busy timeout
- All knot IDs are UUIDv7 (time-ordered)
