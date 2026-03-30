# domain

Pure value types shared across the codebase. No I/O, no side effects.

## Key Files

- **`knot_type.rs`** — `KnotType` enum: `Work` or `Gate`
- **`gate.rs`** — `GateData`, `GateDecision`, `GateOwnerKind`
- **`lease.rs`** — `LeaseData`, `LeaseStatus`
- **`metadata.rs`** — `MetadataEntry`, `MetadataEntryInput`
- **`invariant.rs`** — `Invariant` struct for gate constraints
- **`step_history.rs`** — `StepRecord` for audit trails
- **`state.rs`** — state-related types and parsing
