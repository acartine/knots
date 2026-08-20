---
"knots": patch
---

Make first-use machine id persistence atomic so concurrent processes on a fresh store converge on the same id instead of silently diverging, and surface persist failures as errors instead of discarding them
