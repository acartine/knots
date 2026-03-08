# Implementation Review

## Input
- Knot in `ready_for_implementation_review` state
- Feature branch with implementation
- Knot description and acceptance criteria (use acceptance criteria when
  supplied; otherwise use the description)

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

## Actions
1. Review code changes against the knot description and acceptance
   criteria
2. Verify the implementation respects all knot invariants
3. Verify tests cover the required behavior
4. Verify all sanity gates pass
5. Validate no security issues or regressions introduced
6. Approve or request changes based only on specification and code drift

## Output
- Approved:
  `kno update <id> --add-handoff-capsule "<review summary>"`
  `--handoff-username <username> --handoff-datetime <date RFC3339>`
  `--handoff-agentname <agentname> --handoff-model <model>`
  `--handoff-version <model_version>`
  `kno next <id> <currentState> --actor-kind agent --agent-name <AGENT_NAME>`
  `--agent-model <AGENT_MODEL> --agent-version <AGENT_VERSION>`
- Needs changes:
  `kno update <id> --status ready_for_implementation`
  `--add-note "<feedback>"`
  `kno update <id> --add-handoff-capsule "<enumerated violations of the`
  `knot description and/or acceptance criteria>"`
  `--handoff-username <username> --handoff-datetime <date RFC3339>`
  `--handoff-agentname <agentname> --handoff-model <model>`
  `--handoff-version <model_version>`

## Failure Modes
- Critical issues found:
  `kno update <id> --status ready_for_implementation`
  `--add-note "<feedback>"`
  `kno update <id> --add-handoff-capsule "<enumerated violations of the`
  `knot description and/or acceptance criteria>"`
  `--handoff-username <username> --handoff-datetime <date RFC3339>`
  `--handoff-agentname <agentname> --handoff-model <model>`
  `--handoff-version <model_version>`
- Architecture concern:
  `kno update <id> --status ready_for_implementation`
  `--add-note "<feedback>"`
  `kno update <id> --add-handoff-capsule "<enumerated violations of the`
  `knot description and/or acceptance criteria>"`
  `--handoff-username <username> --handoff-datetime <date RFC3339>`
  `--handoff-agentname <agentname> --handoff-model <model>`
  `--handoff-version <model_version>`
