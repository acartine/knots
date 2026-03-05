# Implementation Review

## Input
- Knot in `ready_for_implementation_review` state
- Feature branch with implementation

## Write Constraints
- Review work is read-only for repository code and git state.
- Do not edit code, tests, docs, configs, or other repository files.
- Do not run git write operations (`git add`, `git commit`, `git merge`, `git rebase`,
  `git push`, `git checkout -b`, etc.).
- Allowed writes are knot metadata updates only (`kno update`
  notes/handoff_capsules/tags).
- If code/git writes are needed to complete review, stop and use the reject/failure path to
  move the knot back to a prior queue state.

## Actions
1. Review code changes for correctness and style
2. Verify tests cover the requirements
3. Verify all sanity gates pass
4. Validate no security issues or regressions introduced
5. Approve or request changes

## Output
- Approved:
  `kno next <id> <currentState> --actor-kind agent --agent-name <AGENT_NAME>`
  `--agent-model <AGENT_MODEL> --agent-version <AGENT_VERSION>`
- Needs changes: `kno update <id> --status ready_for_implementation --add-note "<feedback>"`

## Failure Modes
- Critical issues found: `kno update <id> --status ready_for_implementation --add-note "<feedback>"`
- Architecture concern: `kno update <id> --status ready_for_implementation --add-note "<feedback>"`
