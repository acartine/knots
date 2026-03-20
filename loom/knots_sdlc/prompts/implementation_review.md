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

params: {}
---

# Implementation Review

Review the implementation against the knot description and acceptance criteria.

## Actions

1. Review code changes against the knot description and acceptance criteria
2. Verify the implementation respects all knot invariants
3. Verify tests cover the required behavior
4. Approve or request changes
