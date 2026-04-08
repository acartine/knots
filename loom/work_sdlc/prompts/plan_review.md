---
accept:
  - Plan is complete, correct, and feasible
  - Test strategy covers requirements
  - No security, performance, or maintainability concerns
  - All invariants respected

success:
  approved: ready_for_implementation

failure:
  plan_flawed: ready_for_planning
  requirements_changed: ready_for_planning
  blocked_by_dependency: blocked

params: {}
---

# Plan Review

Review the implementation plan for completeness, correctness, and feasibility.

## Actions

1. Review the plan for completeness, correctness, and feasibility
2. Verify the plan respects all knot invariants
3. Verify test strategy covers requirements
4. Approve or request revisions
