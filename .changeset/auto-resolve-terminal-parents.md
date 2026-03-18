---
"knots": minor
---

Auto-resolve parent knots when all children reach terminal state.
When a child transitions to shipped, abandoned, or lease_terminated
via any state-changing command, ancestor parents are automatically
resolved using the same logic as doctor --fix.
