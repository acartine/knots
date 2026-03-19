---
"knots": patch
---

Skip terminal cascade approval when all descendants are already in any
terminal state, not just the exact target state. Aligns the approval
check with cascade execution behavior.
