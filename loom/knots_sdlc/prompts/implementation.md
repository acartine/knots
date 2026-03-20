---
accept:
  - Working implementation on feature branch
  - All tests passing with coverage threshold met
  - All invariants respected in the implementation
  - Commits tagged on the knot

success:
  implementation_complete: ready_for_implementation_review

failure:
  blocked_by_dependency: deferred
  implementation_infeasible: ready_for_planning
  merge_conflict: ready_for_implementation

params:
  output_kind:
    type: enum
    values: ["local", "remote", "remote_main", "pr"]
    required: true
    description: Artifact output type from the profile
---

# Implementation

Implement the approved plan on a feature branch.

## Actions

1. Create a feature branch from main in a worktree
2. Implement changes following the plan while respecting all invariants
3. Write tests for all new behavior
4. Commit and push the feature branch

## Output

- Working implementation on feature branch
- All tests passing with coverage threshold met
- Handoff capsule with implementation summary
