# Implementation

## Input
- Knot in `ready_for_implementation` state
- Approved implementation plan (in knot notes)

## Actions
1. Create a feature branch from main in a worktree
2. Implement changes following the plan
3. Write tests for all new behavior
4. Run any sanity gates defined in the project or the plan
5. Add a handoff_capsule to the knot with `kno update <id> --add-handoff-capsule"<handoff_capsule>"
 --handoff-username <username> --handoff-date <date RFC3339> --handoff-agentname <agentname> --handoff-model <model> --handoff-version <model_version   >`
5. Commit and push the feature branch
6. Tag the knot with each commit hash using the `commit:` prefix:
   `kno update <id> --add-tag "commit:<full-40-char-hash>"`
   Run this for **every** commit created during implementation.
   Always use the full 40-character hash, not an abbreviated form.
7. Profile variant: Create a PR if the knot profile expects it
8. Profile variant: Merge the feature branch into main if the knot profile expects it

## Output
- Working implementation on feature branch
- All tests passing with coverage threshold met
- Transition: `kno next <id> --actor-kind agent --agent-name <AGENT_NAME> --agent-model <AGENT_MODEL> --agent-version <AGENT_VERSION>`

## Failure Modes
- Blocked by dependency: `kno update <id> --status deferred --add-note "<blocker details>"`
- Implementation infeasible: `kno update <id> --status ready_for_planning --add-note "<blocker details>"`
