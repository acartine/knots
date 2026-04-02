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

Merge the approved implementation using the shipment flow required by
the profile output mode.

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
