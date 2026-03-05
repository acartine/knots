# Shipment

## Input
- Knot in `ready_for_shipment` state
- Approved implementation on feature branch

## Invariant Adherence
- If the knot has invariants, verify they still hold after merge and
  before pushing to remote.
- Scope invariants: confirm no out-of-scope changes leaked into the
  merge.
- State invariants: confirm the required properties hold in the merged
  code on main.

## Actions
1. Profile variant: Merge feature branch to main if the knot profile
   expects it
2. Tag the knot with any new commit hashes created during merge using
   the `commit:` prefix:
   `short_hash=$(git rev-parse --short=12 <commit>)`
   `kno update <id> --add-tag "commit:${short_hash}"`
   Run this for each new commit created during shipment.
   Use short hashes only; do not use the full 40-character hash.
3. Profile variant: Push main to remote if the knot profile expects it
4. Verify CI passes on remote

## Output
- Code merged and pushed to main
- CI green on remote
- Add a handoff capsule summarizing shipment:
  `kno update <id> --add-handoff-capsule "<handoff_capsule>"`
  `--handoff-username <username> --handoff-datetime <date RFC3339>`
  `--handoff-agentname <agentname> --handoff-model <model>`
  `--handoff-version <model_version>`
- Transition:
  `kno next <id> <currentState> --actor-kind agent --agent-name <AGENT_NAME>`
  `--agent-model <AGENT_MODEL> --agent-version <AGENT_VERSION>`

## Failure Modes
- Merge conflicts:
  `kno update <id> --status ready_for_implementation`
  `--add-note "<blocker details>"`
  `kno update <id> --add-handoff-capsule "<merge conflict details>"`
  `--handoff-username <username> --handoff-datetime <date RFC3339>`
  `--handoff-agentname <agentname> --handoff-model <model>`
  `--handoff-version <model_version>`
- CI failure after merge:
  `kno update <id> --status ready_for_implementation`
  `--add-note "<blocker details>"`
  `kno update <id> --add-handoff-capsule "<CI failure details>"`
  `--handoff-username <username> --handoff-datetime <date RFC3339>`
  `--handoff-agentname <agentname> --handoff-model <model>`
  `--handoff-version <model_version>`
