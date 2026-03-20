---
accept:
  - Actionable implementation steps with clear deliverables
  - Scope estimated with complexity assessment
  - Dependencies and risks identified
  - Test strategy covers requirements
  - All invariants respected in the plan

success:
  plan_complete: ready_for_plan_review

failure:
  insufficient_context: ready_for_planning
  out_of_scope: ready_for_planning

params:
  complexity:
    type: enum
    values: ["small", "medium", "large"]
    required: false
    description: Expected implementation complexity
---

# Planning

Break the knot into actionable implementation steps.

## Invariant Adherence

- If the knot has invariants, read and understand each one before planning.
- Every step in the plan must respect all invariant conditions.

## Actions

1. Analyze the knot requirements and constraints
2. Draft an implementation plan with steps, file changes, and test strategy
3. Estimate complexity and identify risks
4. Write the plan as a knot note

## Output

- Detailed implementation plan attached as a knot note
- Hierarchy of knots created
- Handoff capsule summarizing the plan
