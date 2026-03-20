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

params: {}
---

# Shipment Review

Verify the shipped code is live, correct, and regression-free.

## Actions

1. Verify the change is live on main branch
2. Confirm every commit is tagged on the knot
3. Verify all knot invariants hold in the shipped code
4. Confirm CI/CD pipeline completed successfully
5. Final sign-off
