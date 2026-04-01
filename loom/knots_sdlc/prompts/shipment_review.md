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

Verify the shipped result is correct at the review target required by
the profile output mode.

## Actions

1. Verify the shipped result at the correct review target for the
   profile output mode:
   `{{ output }}` = `remote_main` means review the code now on main.
   `{{ output }}` = `pr` means review the merged pull request as the
   shipment record and confirm the corresponding code is now on main.
2. Confirm every commit is tagged on the knot
3. Verify all knot invariants hold in the shipped code
4. Confirm CI/CD pipeline completed successfully
5. Final sign-off
