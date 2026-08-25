# docs

User-facing documentation for `kno` features. These are reference docs, not internal design notes.

## Key Files

- **`portfolio-plan.md`** — how knots aggregate into portfolios, prioritization, and planning loops
- **`execution-plans.md`** — authoring and editing wave/step execution plans on a knot
- **`leases.md`** — lease lifecycle: acquire, extend, terminate, handoff semantics
- **`tier-balance.md`** — hot / warm / cold tier invariants and how `kno doctor --fix` restores them
- **`event-log-compaction.md`** — safe bounded-retention protocol, benchmarks, and rollout plan

Internal design notes and architecture live in the repo root (`ARCHITECTURE.md`, `knots-architecture-v4.md`) rather than here.
