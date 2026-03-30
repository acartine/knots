# app

Core business logic for knot operations.

## Key Files

- **`knot_create.rs`** — `create_knot_with_options()`: new knot creation
- **`knot_update.rs`** — `update_knot_with_options()`: field updates (title, body, tags, etc.)
- **`state_ops.rs`** — `set_state()`, `write_state_change_locked()`: workflow state transitions
- **`gate.rs`** — `evaluate_gate()`: gate review decisions and failure routing
- **`gate_metadata.rs`** — `append_gate_failure_metadata_locked()`: gate failure tracking
- **`edges.rs`** — `apply_edge_change()`: parent/child and dependency edges
- **`query.rs`** — `get_knot()`, `list_knots()`: read operations
- **`rehydrate.rs`** — `rehydrate_from_events()`: rebuild state from event log
- **`types.rs`** — `KnotView`, `EdgeView`, `ChildSummary`, `AppError`

## Key Types

- `App` — main facade; holds SQLite connection, EventWriter, ProfileRegistry
- `KnotView` — full knot representation returned by all operations
- `AppError` — error enum for all app-layer failures
