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
  blocked_by_dependency: blocked

params:
  complexity:
    type: enum
    values: ["small", "medium", "large"]
    required: false
    description: Expected implementation complexity
---

# Planning

## Input
- Knot in `ready_for_planning` state
- Knot title, description, and any existing notes/context

## Invariant Adherence
- If the knot has invariants, read and understand each one before planning.
- Every step in the plan must respect all invariant conditions.
- Scope invariants constrain what the work may touch.
- State invariants constrain what must remain true throughout execution.
- If any planned step would violate an invariant, redesign the approach or
  flag the conflict in the plan note.

## Step Boundary
- This session is authorized only for `planning`.
- Complete exactly one planning action, then stop.
- Allowed resting states after this session: `ready_for_plan_review` on
  success, `ready_for_planning` for retry, or `blocked` when dependencies stop
  progress.
- Creating child knots is planning output only. Do not claim, start, or
  execute those child knots in this session.
- Do not edit repository code or perform git write operations during
  planning.
- After the note, handoff, and transition commands for this step succeed,
  stop immediately.

## Hierarchy Gate
- The parent's transition out of `planning` is gated on every direct
  child's effective state rank meeting or exceeding the parent's
  target state rank.
- A child created with the default `autopilot` profile starts in
  `planning` (rank 1), which blocks the parent's `plan_review`
  transition (rank 3).
- Child knots created during planning MUST therefore start in a
  no-planning profile so they begin at `implementation` (rank 5) and
  do not block the parent's `plan_review` transition.

## Actions
1. Analyze the knot requirements and constraints
2. Review knot invariants and ensure the plan respects them
3. Research relevant code, dependencies, and prior art
4. Draft an implementation plan with steps, file changes, and test strategy
5. Estimate complexity and identify risks
6. Write the plan as a knot note via `kno update <id> --add-note "<plan>"`
7. If the plan requires child knots, create them with a no-planning
   profile so they do not block this parent's `plan_review` transition,
   then link them to this parent. Use any of the equivalent forms below:
   - `kno q "<title>"` — quick-create with the configured default
     quick profile (typically `autopilot_no_planning`).
   - `kno new "<title>" -f` — `--fast` form, same default quick
     profile as `kno q`.
   - `kno new "<title>" -p autopilot_no_planning` — explicit
     no-planning profile.
   Do NOT use the default `autopilot` profile for child knots created
   during planning: a child in `planning` (rank 1) blocks the parent's
   `plan_review` transition (rank 3) via the hierarchy gate.
   Recommended order: create each child knot unlinked first, then
   attach it to this parent with
   `kno edge add <parent-id> parent_of <child-id>`.

## Output
- Detailed implementation plan attached as a knot note
- Hierarchy of knots created
- Add a handoff capsule summarizing the plan:
  `kno update <id> --add-handoff-capsule "<handoff_capsule>"`
- Transition:
  `kno next <id> <currentState> --lease <LEASE_ID>`

## Failure Modes
Record the outcome with a single command. `kno rollback --outcome` moves the
knot to the outcome's workflow-declared target state, closes the step as
failed, and terminates and unbinds the lease in one atomic operation. Attach
the note and handoff capsule first, while the lease is still bound.
- Insufficient context (`insufficient_context` -> `ready_for_planning`):
  `kno update <id> --add-note "<note>"`
  `kno update <id> --add-handoff-capsule "<reason for deferral>"`
  `kno rollback <id> --outcome insufficient_context --lease <LEASE_ID>`
- Out of scope / too complex (`out_of_scope` -> `ready_for_planning`):
  `kno update <id> --add-note "<note>"`
  `kno update <id> --add-handoff-capsule "<reason out of scope>"`
  `kno rollback <id> --outcome out_of_scope --lease <LEASE_ID>`
- Blocked by a dependency (`blocked_by_dependency` -> `blocked`):
  `kno update <id> --add-note "<blocker details>"`
  `kno update <id> --add-handoff-capsule "<blocking dependency details>"`
  `kno rollback <id> --outcome blocked_by_dependency --lease <LEASE_ID>`
