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

Review the implementation plan for completeness, correctness, and
feasibility.

## Input
- Knot in `ready_for_plan_review` state
- Implementation plan from the planning phase (in knot notes)

## Write Constraints
- Review work is read-only for repository code and git state.
- Do not edit code, tests, docs, configs, or other repository files.
- Do not run git write operations (`git add`, `git commit`, `git merge`,
  `git rebase`, `git push`, `git checkout -b`, etc.).
- Allowed writes are knot metadata updates only (`kno update`
  notes/handoff_capsules/tags).
- If code/git writes are needed to complete review, stop and use the
  reject/failure path to move the knot back to a prior queue state.

## Invariant Review
- If the knot has invariants, verify the plan does not violate any of them.
- For each invariant, confirm the planned steps respect the condition.
- Reject the plan if any step would breach a scope or state invariant.

## Step Boundary
- This session is authorized only for `plan_review`.
- Complete exactly one review action, then stop.
- Allowed resting states after this session: `ready_for_implementation` or
  `ready_for_planning`.
- Do not start implementation work after approving the plan.
- After the review decision, handoff, and transition commands succeed, stop
  immediately.

## Actions
1. Review the plan for completeness, correctness, and feasibility
2. Verify the plan respects all knot invariants
3. Verify test strategy covers requirements
4. Check for security, performance, and maintainability concerns
5. Approve or request revisions

## Output
- Approved:
  `kno update <id> --add-handoff-capsule "<review summary>"`
  `kno next <id> <currentState> --lease <LEASE_ID>`
- Needs revision:
  `kno update <id> --status ready_for_planning --add-note "<feedback>"`
  `kno update <id> --add-handoff-capsule "<revision needed>"`

## Failure Modes
Record the outcome with a single command. `kno rollback --outcome` moves the
knot to the outcome's workflow-declared target state, closes the step as
failed, and terminates and unbinds the lease in one atomic operation. Attach
the note and handoff capsule first, while the lease is still bound.
- Plan fundamentally flawed (`plan_flawed` -> `ready_for_planning`):
  `kno update <id> --add-note "<feedback>"`
  `kno update <id> --add-handoff-capsule "<plan flawed>"`
  `kno rollback <id> --outcome plan_flawed --lease <LEASE_ID>`
- Requirements changed (`requirements_changed` -> `ready_for_planning`):
  `kno update <id> --add-note "<feedback>"`
  `kno update <id> --add-handoff-capsule "<requirements changed>"`
  `kno rollback <id> --outcome requirements_changed --lease <LEASE_ID>`
- Blocked by a dependency (`blocked_by_dependency` -> `blocked`):
  `kno update <id> --add-note "<blocker details>"`
  `kno update <id> --add-handoff-capsule "<blocking dependency details>"`
  `kno rollback <id> --outcome blocked_by_dependency --lease <LEASE_ID>`
