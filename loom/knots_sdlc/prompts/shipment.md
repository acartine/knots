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

params:
  output_kind:
    type: enum
    values: ["local", "remote", "remote_main", "pr"]
    required: true
    description: Artifact output type from the profile
---

# Shipment

Merge the approved implementation to main and push to remote.

## Actions

1. Merge feature branch to main if the profile output kind requires it
2. Push main to remote if the profile output kind requires it
3. Verify CI passes on remote

## Output

- Code merged and pushed to main
- CI green on remote
- Handoff capsule summarizing shipment
