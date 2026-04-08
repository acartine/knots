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

Review the implementation against the knot description and acceptance
criteria. The implementation has already been built — your job is to
verify it meets the specification, not to re-implement or extend it.

## Locating the Implementation

Find the review artifact by reading the knot metadata:
1. Check knot tags for the artifact identifier:
   `{{ output }}` = `remote_main` means look for a `branch:` tag
   naming the feature branch.
   `{{ output }}` = `pr` means look for a `pr:` tag with the PR
   number.
   `{{ output }}` = `branch` means look for a `branch:` tag naming
   the feature branch.
   `{{ output }}` = `live_deployment` means look for a `branch:` tag
   naming the feature branch.
2. Check `commit:` tags — these are the implementation commit hashes.
3. Read the most recent handoff capsules for the artifact location.

## Actions

1. Locate the review artifact using the steps above
2. Review code changes against the knot description and acceptance
   criteria:
   `{{ output }}` = `remote_main` means review the branch diff against
   main, check test results, and verify sanity gates pass.
   `{{ output }}` = `pr` means review the pull request diff, status,
   CI checks, and PR metadata.
   `{{ output }}` = `branch` means review the branch diff and test
   results as the final deliverable.
   `{{ output }}` = `live_deployment` means review for deployment
   readiness, including infrastructure and rollback considerations.
3. Verify the implementation respects all knot invariants
4. Verify tests cover the required behavior
5. Approve or request changes
