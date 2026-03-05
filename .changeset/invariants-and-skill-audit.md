---
"knots": minor
---

### Features
- Add invariants field to knots with full lifecycle support: Invariant type
  (Scope/State), event sourcing, SQLite v7 migration, CLI flags
  (`--add-invariant`, `--remove-invariant`, `--clear-invariants`), UI display,
  and prompt rendering.

### Fixes
- Relax self_manage test assertion for coverage tool compatibility.
- Fix `--handoff-date` to `--handoff-datetime` in skill templates.

### Docs
- Add handoff capsules with full agent metadata to all skill prompt paths
  (success and failure modes).

### Tests
- Add invariant CLI flag, model serialization, persistence round-trip, and
  sync/apply coverage tests.
- Improve coverage collection and integration test binary resolution.
