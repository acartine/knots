---
"knots": minor
---

Introduce the protocol-v2 compaction trust boundary and its required local persistence changes.

- Fail closed when sync encounters protocol-v2 state, preventing publication until provider
  protection is validated.
- Migrate local SQLite caches to durable outboxes and inventory legacy event files for replay.
- Add authenticated-checkpoint and writer-identity persistence, with private permissions for
  SQLite files and sidecars.
- Add signing and verification primitives for immutable proposal submissions.
- Enforce zero-bypass immutable GitHub refs and OIDC-attested control epochs.

Upgrade all writers and readers before enabling the separately staged legacy-ref freeze.
