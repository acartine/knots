# Shipment

## Input
- Knot in `ready_for_shipment` state
- Approved implementation on feature branch

## Actions
1. Profile variant: Merge feature branch to main if the knot profile expects it
2. Tag the knot with any new commit hashes created during merge using the
   `commit:` prefix:
   `kno update <id> --add-tag "commit:<full-40-char-hash>"`
   Run this for each new commit created during shipment.
   Always use the full 40-character hash, not an abbreviated form.
3. Profile variant: Push main to remote if the knot profile expects it
4. Verify CI passes on remote

## Output
- Code merged and pushed to main
- CI green on remote
- Transition:
  `kno next <id> <currentState> --actor-kind agent --agent-name <AGENT_NAME>`
  `--agent-model <AGENT_MODEL> --agent-version <AGENT_VERSION>`

## Failure Modes
- Merge conflicts:
  `kno update <id> --status ready_for_implementation --add-note "<blocker details>"`
- CI failure after merge:
  `kno update <id> --status ready_for_implementation --add-note "<blocker details>"`
