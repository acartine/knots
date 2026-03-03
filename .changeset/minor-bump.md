---
"knots": minor
---

Add structured JSON output and agent metadata to kno next

- `kno next` now supports `--json` to emit structured JSON containing
  the knot id, previous state, new state, and owner_kind.
- All `kno next` calls in skill prompts include agent metadata flags
  (`--actor-kind`, `--agent-name`, `--agent-model`, `--agent-version`).
- Agent metadata is included in claim completion commands.
- Eliminated unsafe env var manipulation from all tests in favor of
  injectable overrides.
- Fixed sync test time drift by widening hot_window_days.
