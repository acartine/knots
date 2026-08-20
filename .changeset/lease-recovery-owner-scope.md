---
"knots": patch
---

Never recover a lease another machine holds, and keep the cache lock off the
push half of `kno sync`.

`lease_expiry_ts` is not replicated, so a live lease pulled from another
machine arrived looking long expired. Expiry recovery would then terminate it,
roll its knot back, and let a second machine claim work that was still running
elsewhere. Recovery is now owner-scoped, matching the sync guard.

`kno sync` also no longer holds the cache lock across the network push, so a
slow push cannot make a concurrent `kno update` or `kno next` fail with
`lock busy`.
