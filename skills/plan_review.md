# Plan Review

## Input
- Knot in `ready_for_plan_review` state
- Implementation plan from the planning phase (in knot notes)

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
1. Review the plan for completeness, correctness, and feasibility
2. Verify test strategy covers requirements
3. Check for security, performance, and maintainability concerns
4. Approve or request revisions

## Output
- Approved:
  `kno next <id> <currentState> --actor-kind agent --agent-name <AGENT_NAME>`
  `--agent-model <AGENT_MODEL> --agent-version <AGENT_VERSION>`
- Needs revision: `kno update <id> --status ready_for_planning --add-note "<feedback>"`

## Failure Modes
- Plan fundamentally flawed: `kno update <id> --status ready_for_planning --add-note "<feedback>"`
- Requirements changed: `kno update <id> --status ready_for_planning --add-note "<feedback>"`
