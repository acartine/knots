# Plan Review

## Input
- Knot in `ready_for_plan_review` state
- Implementation plan from the planning phase (in knot notes)

## Actions
1. Review the plan for completeness, correctness, and feasibility
2. Verify test strategy covers requirements
3. Check for security, performance, and maintainability concerns
4. Approve or request revisions

## Output
- Approved: `kno state <id> ready_for_implementation`
- Needs revision: `kno state <id> ready_for_planning` with review notes

## Failure Modes
- Plan fundamentally flawed: `kno state <id> ready_for_planning` with feedback
- Requirements changed: `kno state <id> ready_for_planning` with updated context
