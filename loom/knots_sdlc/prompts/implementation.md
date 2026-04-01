---
accept:
  - Working implementation on feature branch
  - All tests passing with coverage threshold met
  - All invariants respected in the implementation
  - Commits tagged on the knot

success:
  implementation_complete: ready_for_implementation_review

failure:
  blocked_by_dependency: blocked
  implementation_infeasible: ready_for_planning
  merge_conflict: ready_for_implementation

params:
  output:
    type: enum
    values: [remote_main, pr]
---

# Implementation

Implement the approved plan on a feature branch.

## Actions

1. Create a feature branch from main in a worktree
2. Implement changes following the plan while respecting all invariants
3. Write tests for all new behavior
4. Commit and push the feature branch
5. Make the implementation artifact explicit for the profile output mode:
   `{{ output }}` = `remote_main` means the review target is the pushed
   feature branch itself, so leave the result ready for direct branch
   review.
   `{{ output }}` = `pr` means the review target is a pull request, so
   open or update the PR for the feature branch.

## Output

The expected output artifact is `{{ output }}`:
- **remote_main**: a feature branch pushed to remote for direct branch review
- **pr**: a pull request opened or updated from the feature branch
