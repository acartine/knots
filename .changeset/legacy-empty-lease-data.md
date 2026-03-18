---
"knots": patch
---

Fix upgraded installs that still have legacy `{}` lease cache payloads so
commands like `show --json` continue to work after the lease metadata schema
expands.
