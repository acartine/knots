---
accept:
  - Working implementation on feature branch
  - All tests passing with coverage threshold met
  - All invariants respected in the implementation
  - Commits tagged on the knot
  - Artifact identifier (branch name or PR number) tagged and in handoff capsule

success:
  implementation_complete: ready_for_implementation_review

failure:
  blocked_by_dependency: blocked
  implementation_infeasible: ready_for_planning
  merge_conflict: ready_for_implementation

params:
  output:
    type: enum
    values: [remote_main, pr, branch, live_deployment]
---

# Implementation

Implement the approved plan on a feature branch.

## Actions

1. Create a feature branch from main in a worktree
2. Implement changes following the plan while respecting all invariants
3. Write tests for all new behavior
4. Commit and push the feature branch
5. Create the review artifact required by the profile output mode:
   `{{ output }}` = `remote_main` means push the feature branch to
   remote. The branch itself is the review artifact.
   `{{ output }}` = `pr` means open a pull request from the feature
   branch. The PR is the review artifact.
   `{{ output }}` = `branch` means push the feature branch to remote.
   The branch is the final deliverable (no merge to main expected).
   `{{ output }}` = `live_deployment` means prepare the implementation
   for deployment review.

## Tagging the Artifact

After creating the review artifact, tag the knot so downstream steps
can locate the work:

1. Tag each commit hash with the `commit:` prefix.
2. Tag the artifact identifier so reviewers can find it:
   `{{ output }}` = `remote_main` means tag the branch name with
   `kno update <id> --add-tag "branch:<branch-name>"`.
   `{{ output }}` = `pr` means tag the PR number with
   `kno update <id> --add-tag "pr:<number>"`.
   `{{ output }}` = `branch` means tag the branch name with
   `kno update <id> --add-tag "branch:<branch-name>"`.
   `{{ output }}` = `live_deployment` means tag the branch name with
   `kno update <id> --add-tag "branch:<branch-name>"`.
3. Include the artifact identifier in the handoff capsule so the
   reviewer knows exactly where to look (e.g., "Branch:
   worktree-knots-1234-feature" or "PR #42").

## Output

The expected output artifact is `{{ output }}`:
- **remote_main**: a feature branch pushed to remote for branch review
- **pr**: a pull request opened from the feature branch
- **branch**: a feature branch pushed to remote as the final deliverable
- **live_deployment**: implementation ready for deployment review
