---
"knots": patch
---

Add a `nested_caches` doctor check that warns when cached `.knots` directories
are nested inside the canonical store. The warning lists each detected cache and
prints the manual `rm -rf <path>` cleanup command so users can resolve silent
state-drift risks deliberately.
