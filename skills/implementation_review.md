# Implementation Review

## Input
- Knot in `ready_for_implementation_review` state
- Implementation artifact (branch or PR) tagged on the knot
- Knot description and acceptance criteria (use acceptance criteria when
  supplied; otherwise use the description)

## Locating the Implementation
Find the review artifact by reading the knot metadata:
1. Check knot tags for the artifact identifier:
   `{{ output }}` = `remote_main` means look for a `branch:` tag
   naming the feature branch.
   `{{ output }}` = `pr` means look for a `pr:` tag with the PR
   number.
2. Check `commit:` tags — these are the implementation commit hashes.
3. Read the most recent handoff capsules for the artifact location.
4. If no artifact tag exists, use `git branch -a --contains <commit>`
   on a tagged commit to find the branch.

## Write Constraints
- Review work is read-only for repository code and git state.
- Do not edit code, tests, docs, configs, or other repository files.
- Do not run git write operations (`git add`, `git commit`, `git merge`,
  `git rebase`, `git push`, `git checkout -b`, etc.).
- Allowed writes are knot metadata updates only (`kno update`
  notes/handoff_capsules/tags).
- If code/git writes are needed to complete review, stop and use the
  reject/failure path to move the knot back to a prior queue state.

## Invariant Review
- If the knot has invariants, verify the implementation does not violate
  any of them.
- For each scope invariant, confirm changes are limited to the allowed
  scope.
- For each state invariant, confirm the required property holds in the
  implemented code.
- Reject the implementation if any invariant condition is breached.

## Review Basis
- Base approval strictly on the code under review and the knot
  description plus acceptance criteria.
- Treat the acceptance criteria as the source of truth when they are
  present; otherwise use the description as the requirement baseline.
- Do not use knot notes or prior handoff_capsules to decide whether the
  implementation is approved.
- Use notes or handoff_capsules only as supplemental context when
  locating the implementation or understanding prior workflow history.

## Step Boundary
- This session is authorized only for `implementation_review`.
- Complete exactly one review action, then stop.
- Allowed resting states after this session: `ready_for_shipment` or
  `ready_for_implementation`.
- Do not patch code, amend commits, or continue into shipment after a
  review decision.
- After the review decision, handoff, and transition commands succeed,
  stop immediately.

## Actions
1. Locate the review artifact using the steps above
2. Review code changes against the knot description and acceptance
   criteria:
   `{{ output }}` = `remote_main` means review the branch diff against
   main, check test results, and verify sanity gates pass.
   `{{ output }}` = `pr` means review the pull request diff, status,
   CI checks, and PR metadata.
3. Verify the implementation respects all knot invariants
4. Verify tests cover the required behavior
5. Verify all sanity gates pass
6. Validate no security issues or regressions introduced
7. Approve or request changes based only on specification and code drift

## Output
- Approved:
  `kno update <id> --add-handoff-capsule "<review summary>"`
  `kno next <id> <currentState> --actor-kind agent --agent-name <AGENT_NAME>`
  `--agent-model <AGENT_MODEL> --agent-version <AGENT_VERSION>`
- Needs changes:
  `kno update <id> --status ready_for_implementation`
  `--add-note "<feedback>"`
  `kno update <id> --add-handoff-capsule "<enumerated violations of the`
  `knot description and/or acceptance criteria>"`

## Failure Modes
- Critical issues found:
  `kno update <id> --status ready_for_implementation`
  `--add-note "<feedback>"`
  `kno update <id> --add-handoff-capsule "<enumerated violations of the`
  `knot description and/or acceptance criteria>"`
- Architecture concern:
  `kno update <id> --status ready_for_implementation`
  `--add-note "<feedback>"`
  `kno update <id> --add-handoff-capsule "<enumerated violations of the`
  `knot description and/or acceptance criteria>"`
