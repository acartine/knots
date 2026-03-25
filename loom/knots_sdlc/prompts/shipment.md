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
  release_blocked: deferred

params: {}
---

# Shipment

Merge the approved implementation to main and push to remote.

## Actions

1. Merge the feature branch to main
2. Push main to remote
3. Verify CI passes on remote

## Output

The expected output artifact is a **commit** on main:
- Code merged and pushed to main
- CI green on remote
- Handoff capsule summarizing shipment
