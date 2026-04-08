---
accept: []

success:
  gate_passed: shipped

failure:
  gate_failed: abandoned

params: {}
---

# Evaluating

Assess the gate as a binary yes/no decision.

## Step Boundary

- This session is authorized only for `evaluating`.
- Complete exactly one evaluation action, then stop.
- Allowed resting states after this session: `shipped` or `abandoned`.
- If the gate passes, use the completion command below.
- If the gate fails, run `kno gate evaluate <id> --decision no --invariant "<violated invariant>"`.
- After a listed completion or failure-path command succeeds, stop immediately.

## Actions

1. Review the gate description, invariants, and failure modes.
2. Decide whether the gate passes or fails.
3. On pass, use the completion command below to move the gate to `shipped`.
4. On failure, record the violated invariant with `kno gate evaluate ...`.
5. Do not continue into follow-up work after the gate decision is recorded.
