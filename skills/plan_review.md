# Plan Review

## Input
- Knot in `ready_for_plan_review` state
- Implementation plan from the planning phase (in knot notes)

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
- If the knot has invariants, verify the plan does not violate any of them.
- For each invariant, confirm the planned steps respect the condition.
- Reject the plan if any step would breach a scope or state invariant.

## Step Boundary
- This session is authorized only for `plan_review`.
- Complete exactly one review action, then stop.
- Allowed resting states after this session: `ready_for_implementation` or
  `ready_for_planning`.
- Do not start implementation work after approving the plan.
- After the review decision, handoff, and transition commands succeed, stop
  immediately.

## Actions
1. Review the plan for completeness, correctness, and feasibility
2. Verify the plan respects all knot invariants
3. Verify test strategy covers requirements
4. Check for security, performance, and maintainability concerns
5. Approve or request revisions

## Output
- Approved:
  `kno update <id> --add-handoff-capsule "<review summary>"`
  `--handoff-username <username> --handoff-datetime <date RFC3339>`
  `--handoff-agentname <agentname> --handoff-model <model>`
  `--handoff-version <model_version>`
  `kno next <id> <currentState> --actor-kind agent --agent-name <AGENT_NAME>`
  `--agent-model <AGENT_MODEL> --agent-version <AGENT_VERSION>`
- Needs revision:
  `kno update <id> --status ready_for_planning --add-note "<feedback>"`
  `kno update <id> --add-handoff-capsule "<revision needed>"`
  `--handoff-username <username> --handoff-datetime <date RFC3339>`
  `--handoff-agentname <agentname> --handoff-model <model>`
  `--handoff-version <model_version>`

## Failure Modes
- Plan fundamentally flawed:
  `kno update <id> --status ready_for_planning --add-note "<feedback>"`
  `kno update <id> --add-handoff-capsule "<plan flawed>"`
  `--handoff-username <username> --handoff-datetime <date RFC3339>`
  `--handoff-agentname <agentname> --handoff-model <model>`
  `--handoff-version <model_version>`
- Requirements changed:
  `kno update <id> --status ready_for_planning --add-note "<feedback>"`
  `kno update <id> --add-handoff-capsule "<requirements changed>"`
  `--handoff-username <username> --handoff-datetime <date RFC3339>`
  `--handoff-agentname <agentname> --handoff-model <model>`
  `--handoff-version <model_version>`
