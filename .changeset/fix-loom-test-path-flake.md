---
"knots": patch
---

Fix a flaky `make sanity`: loom tests no longer mutate process-global `PATH`, and the machine-id test no longer depends on `KNOTS_MACHINE_ID` being unset in the environment
