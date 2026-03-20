---
"knots": minor
---

Add lease threading to claim and next commands. External lease IDs can now be passed via --lease flags so the calling process can thread its own lease through the workflow instead of having duplicate leases created.
