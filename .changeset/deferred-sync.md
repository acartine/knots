---
"knots": minor
---

Defer `kno sync` when active leases exist instead of erroring. The sync is queued via sync_pending and automatically triggered when the last active lease is terminated.
