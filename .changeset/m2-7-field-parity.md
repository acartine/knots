---
"knots": minor
---

Add M2.7 field parity and migration readiness with:

- `knots update` patch command for title, description, priority, status, type, tags,
  notes, and handoff capsules.
- first-class `notes[]` and `handoff_capsules[]` metadata arrays
  (`username/datetime/agentname/model/version`).
- SQLite migration v3 parity fields and backfill from legacy body/notes.
- import and sync reducers updated for parity mapping and metadata event handling.
