---
accept:
  - Change is live on main branch
  - Every commit tagged on the knot
  - All invariants hold in shipped code
  - CI/CD pipeline completed successfully
  - No regressions in dependent systems

success:
  approved: shipped
  approved_already_merged: shipped

failure:
  needs_revision: ready_for_implementation
  critical_regression: ready_for_implementation
  deployment_issue: ready_for_shipment
  dirty_workspace: ready_for_implementation

params:
  output:
    type: enum
    values: [remote_main, pr, branch, live_deployment]
---

# Shipment Review

Verify the shipped result is correct at the review target required by
the profile output mode.

## Input
- Knot in `ready_for_shipment_review` state
- Code merged to main, CI green

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
- If the knot has invariants, verify the shipped code does not violate
  any of them.
- For each scope invariant, confirm only allowed areas were changed.
- For each state invariant, confirm the property still holds on main.
- Reject if any invariant condition is breached.

## Step Boundary
- This session is authorized only for `shipment_review`.
- Complete exactly one review action, then stop.
- Allowed resting states after this session: `shipped`,
  `ready_for_shipment`, or `ready_for_implementation`.
- Do not fix code, re-run shipment, or continue into other workflow
  stages in this session.
- After the review decision, handoff, and transition commands succeed,
  stop immediately.

## Actions
1. Verify the shipped result at the correct review target for the
   profile output mode:
   `{{ output }}` = `remote_main` means review the code now on main.
   `{{ output }}` = `pr` means review the merged pull request as the
   shipment record and confirm the corresponding code is now on main.
   `{{ output }}` = `branch` means verify the branch is on remote and
   CI passed.
   `{{ output }}` = `live_deployment` means verify the deployment is
   live, healthy, and matches the approved implementation.
2. Confirm every commit from implementation/shipment is tagged on the
   knot:
   - Use the `commit:` prefix for each tag.
   - Each tag must include a short hash from
     `git rev-parse --short=12 <commit>` (not the full 40-character hash).
3. Verify all knot invariants hold in the shipped code
4. Confirm CI/CD pipeline completed successfully
5. Validate no regressions in dependent systems
6. Final sign-off

## Output
- Approved:
  `kno update <id> --add-handoff-capsule "<review summary>"`
  `kno next <id> <currentState> --lease <LEASE_ID>`
- Needs revision:
  `kno update <id> --status ready_for_implementation`
  `--add-note "<blocker details>"`
  `kno update <id> --add-handoff-capsule "<revision needed>"`
- Critical regression:
  `kno update <id> --status ready_for_implementation`
  `--add-note "<blocker details>"`
  `kno update <id> --add-handoff-capsule "<critical regression>"`

## Failure Modes
Record the outcome with a single command. `kno rollback --outcome` moves the
knot to the outcome's workflow-declared target state, closes the step as
failed, and terminates and unbinds the lease in one atomic operation. Attach
the note and handoff capsule first, while the lease is still bound.
- Deployment issue (`deployment_issue` -> `ready_for_shipment`):
  `kno update <id> --add-note "<blocker details>"`
  `kno update <id> --add-handoff-capsule "<deployment issue>"`
  `kno rollback <id> --outcome deployment_issue --lease <LEASE_ID>`
- Regression detected (`critical_regression` -> `ready_for_implementation`):
  `kno update <id> --add-note "<blocker details>"`
  `kno update <id> --add-handoff-capsule "<regression details>"`
  `kno rollback <id> --outcome critical_regression --lease <LEASE_ID>`
- Needs revision (`needs_revision` -> `ready_for_implementation`):
  `kno update <id> --add-note "<feedback>"`
  `kno update <id> --add-handoff-capsule "<requested revisions>"`
  `kno rollback <id> --outcome needs_revision --lease <LEASE_ID>`
- Unable to complete review due to dirty workspace (`dirty_workspace` ->
  `ready_for_implementation`):
  `kno update <id> --add-note "<dirty workspace details>"`
  `kno update <id> --add-handoff-capsule "<dirty workspace handoff>"`
  `kno rollback <id> --outcome dirty_workspace --lease <LEASE_ID>`
