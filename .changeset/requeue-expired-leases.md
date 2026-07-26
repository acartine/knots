---
"knots": patch
---

Requeue action-state knots when their bound lease expires or is explicitly
terminated. Stale actors can safely create a fresh lease and reclaim queued
work, while active replacement leases remain protected.

Document the recovery flow in the managed `knots` and `knots-e2e` skills.
