---
accept:
  - Code matches knot description and acceptance criteria
  - All invariants respected in the implementation
  - Tests cover required behavior
  - All sanity gates pass
  - No security issues or regressions

success:
  approved: ready_for_shipment

failure:
  changes_requested: ready_for_implementation
  architecture_concern: ready_for_implementation
  critical_issues: ready_for_implementation

params:
  output:
    type: enum
    values: [remote_main, pr, branch, live_deployment]
---

# Implementation Review

Review the implementation against the knot description and acceptance criteria.

## Actions

1. Review code changes against the knot description and acceptance criteria
2. Verify the implementation respects all knot invariants
3. Verify tests cover the required behavior
4. Use the correct review target for the profile output mode:
   `{{ output }}` = `remote_main` means review the implementation branch
   directly using the branch diff, status, and test results.
   `{{ output }}` = `pr` means review the pull request itself, including
   the PR diff, status, and metadata.
   `{{ output }}` = `branch` means review the pushed branch as the final
   deliverable, using the branch diff and test results.
   `{{ output }}` = `live_deployment` means review the implementation
   for deployment readiness, including infrastructure and rollback plan.
5. Approve or request changes
