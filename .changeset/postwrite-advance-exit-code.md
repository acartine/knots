---
"knots": patch
---

Make a lease-bound `kno next` exit 0 whenever the state write succeeds.
Failures that happen after the write — display enrichment, or profile
resolution for unrelated knots during the lease-release scan — are now
stderr warnings instead of masking a successful advance with exit 1.

The generic bound-lease guidance error now names the bound lease id
(`knot has a bound lease '<id>'; rerun with --lease <id>`), so scripts no
longer need to probe with a bogus `--lease` to learn it.

Also adds a regression test locking RFC 8259-valid `kno show -j` output
for knots with multiline descriptions and handoff capsules.
