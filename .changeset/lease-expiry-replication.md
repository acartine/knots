---
"knots": patch
---

Replicate `lease_expiry_ts` on `idx.knot_head` so a lease pulled from another machine shows its real expiry instead of always reading as expired; `kno lease ls --all` and `kno doctor` no longer mislabel a live foreign lease as stuck.
