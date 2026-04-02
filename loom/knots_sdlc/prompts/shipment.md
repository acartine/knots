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

## Actions

1. Perform the shipment action that matches the profile output mode:
   `{{ output }}` = `remote_main` means merge the feature branch to main.
   `{{ output }}` = `pr` means merge the approved pull request instead of
   performing a branch-only review flow.
   `{{ output }}` = `branch` means the branch is the final artifact;
   verify it is pushed and CI passes.
   `{{ output }}` = `live_deployment` means merge to main and deploy to
   the target environment.
2. Push or verify the shipped result required by the output mode:
   `{{ output }}` = `remote_main` means push main after the merge.
   `{{ output }}` = `pr` means verify the merged PR produced the intended
   main-branch result and that the remote reflects it.
   `{{ output }}` = `branch` means verify the branch is on remote.
   `{{ output }}` = `live_deployment` means verify the deployment
   completed and the service is healthy.
3. Verify CI passes on remote

## Output

The expected output artifact is `{{ output }}`:
- **remote_main**: code merged from the feature branch to main and
  pushed to remote
- **pr**: an approved pull request merged and reflected on main
- **branch**: feature branch pushed to remote as the final deliverable
- **live_deployment**: code merged, deployed, and verified healthy

## When to Roll Back

Only roll back to `ready_for_implementation` when the merge itself
fails (conflicts, CI red after merge). Finding unmerged commits is
the normal starting condition — that is what shipment is for.
