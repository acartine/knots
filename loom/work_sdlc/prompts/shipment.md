---
accept:
  - Code merged and pushed to main
  - CI green on remote
  - All invariants still hold after merge
  - All commits tagged on the knot

success:
  shipment_complete: ready_for_shipment_review

failure:
  merge_conflicts: ready_for_implementation
  ci_failure: ready_for_implementation
  release_blocked: blocked

params:
  output:
    type: enum
    values: [remote_main, pr, branch, live_deployment]
---

# Shipment

Promote the approved implementation to its final destination. The
implementation has already been reviewed and approved — your job is to
merge, push, and verify, not to re-review or second-guess the work.

## Input
- Knot in `ready_for_shipment` state
- Approved implementation on feature branch

## Purpose
The implementation has been reviewed and approved. Your job is to
promote it to its final destination (merge, push, verify). Do not
re-review or second-guess the approved work.

## Locating the Implementation
Find the feature branch by reading the knot metadata:
1. Check `commit:` tags on the knot — these are the implementation
   commit hashes.
2. Read the most recent handoff capsules — they typically name the
   branch (e.g., `worktree-<knot-id>-*`).
3. Run `git branch -a --contains <tagged-commit>` to confirm which
   branch holds the work.

If the tagged commits are already on main, shipment is already done —
verify CI is green and advance. Do not roll back.

## Invariant Adherence
- If the knot has invariants, verify they still hold after merge and
  before pushing to remote.
- Scope invariants: confirm no out-of-scope changes leaked into the
  merge.
- State invariants: confirm the required properties hold in the merged
  code on main.

## Step Boundary
- This session is authorized only for `shipment`.
- Complete exactly one shipment action, then stop.
- Allowed resting states after this session: `ready_for_shipment_review`
  or `ready_for_implementation`.
- Do not perform shipment review or final sign-off in this step.
- After the merge, push, handoff, and transition commands for shipment
  succeed, stop immediately.

## Actions
1. Locate the feature branch using the steps above
2. Perform the shipment action that matches the profile output mode:
   `{{ output }}` = `remote_main` means merge the feature branch to main.
   `{{ output }}` = `pr` means merge the approved pull request instead of
   performing a branch-only review flow.
   `{{ output }}` = `branch` means the branch is the final artifact;
   verify it is pushed and CI passes.
   `{{ output }}` = `live_deployment` means merge to main and deploy to
   the target environment.
3. Tag the knot with any new commit hashes created during merge using
   the `commit:` prefix:
   `short_hash=$(git rev-parse --short=12 <commit>)`
   `kno update <id> --add-tag "commit:${short_hash}"`
   Run this for each new commit created during shipment.
   Use short hashes only; do not use the full 40-character hash.
4. Push or verify the shipped main-branch result required by the output
   mode:
   `{{ output }}` = `remote_main` means push main after the merge.
   `{{ output }}` = `pr` means verify the merged PR produced the intended
   main-branch result and that the remote reflects it.
   `{{ output }}` = `branch` means verify the branch is on remote.
   `{{ output }}` = `live_deployment` means verify the deployment
   completed and the service is healthy.
5. Verify CI passes on remote

## Output
- Code merged and pushed to main
- CI green on remote
- Add a handoff capsule summarizing shipment:
  `kno update <id> --add-handoff-capsule "<handoff_capsule>"`
- Transition:
  `kno next <id> <currentState> --lease <LEASE_ID>`

## When to Roll Back
Only roll back to `ready_for_implementation` when the merge itself
fails (conflicts, CI red after merge). Finding unmerged commits is
the normal starting condition — that is what shipment is for.

## Failure Modes
Record the outcome with a single command. `kno rollback --outcome` moves the
knot to the outcome's workflow-declared target state, closes the step as
failed, and terminates and unbinds the lease in one atomic operation. Attach
the note and handoff capsule first, while the lease is still bound.
- Merge conflicts (`merge_conflicts` -> `ready_for_implementation`):
  `kno update <id> --add-note "<blocker details>"`
  `kno update <id> --add-handoff-capsule "<merge conflict details>"`
  `kno rollback <id> --outcome merge_conflicts --lease <LEASE_ID>`
- CI failure after merge (`ci_failure` -> `ready_for_implementation`):
  `kno update <id> --add-note "<blocker details>"`
  `kno update <id> --add-handoff-capsule "<CI failure details>"`
  `kno rollback <id> --outcome ci_failure --lease <LEASE_ID>`
- Release blocked (`release_blocked` -> `blocked`):
  `kno update <id> --add-note "<blocker details>"`
  `kno update <id> --add-handoff-capsule "<release blocker details>"`
  `kno rollback <id> --outcome release_blocked --lease <LEASE_ID>`
