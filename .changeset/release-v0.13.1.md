---
"knots": patch
---

- Add lease timeout with lazy expiry and heartbeat
- Restrict lease binding to claim flow
- Hide lease IDs from generic show output
- Improve read tracing, sync dedup, and cache-miss behavior
- Tighten lease enforcement for claims and next
- Materialize expired leases, preserve heartbeat timeout, harden next exception
- Remove auto-sync, fix worktree discovery, and scope lease materialization
- Cover unknown lease state fallback to stabilize coverage
