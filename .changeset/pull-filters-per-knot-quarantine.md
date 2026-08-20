---
"knots": patch
---

`kno pull` and `kno sync` no longer defer wholesale when a local lease is active: they filter per knot instead, applying every event except those for knots this machine currently leases, and quarantine the held-back events for automatic replay once the lease terminates or expires.
