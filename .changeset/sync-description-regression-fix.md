---
"knots": patch
---

Fix `kno sync` dropping descriptions for knots created with `kno new -d`. Create
now emits a separate `knot.description_set` event so the standard apply path
populates description on the receiving host. The sync-apply and rehydrate paths
also gained a backward-compat read of the inline `body` field on `knot.created`
events so descriptions on knots created before this fix are recovered on the
next pull. The compat reads will be removed once the pre-fix event cohort ages
out (tracked by knot `83b1`).
