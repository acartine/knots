# poll_claim

Find and claim the highest-priority ready knot for the current agent.

## Key Files

- **`mod.rs`** — `run_poll()`, `run_claim()`: entry points
- **`ready.rs`** — `run_ready()`, `list_queue_candidates()`: queue filtering

## Behavior

- `kno poll`: peek at next claimable knot
- `kno poll --claim`: claim and return action prompt
- `kno claim`: claim a specific knot by ID
- Respects profile ownership (human vs agent) and lease state
