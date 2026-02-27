# Shipment

## Input
- Knot in `ready_for_shipment` state
- Approved implementation on feature branch

## Actions
1. Profile variant: Merge feature branch to main if the knot profile expects it
2. Profile variant: Push main to remote if the knot profile expects it
3. Verify CI passes on remote

## Output
- Code merged and pushed to main
- CI green on remote
- Transition: `kno next <id>`

## Failure Modes
- Merge conflicts: `kno update <id> --status ready_for_implementation --add-note "<blocker details>"`
- CI failure after merge: `kno update <id> --status ready_for_implementation --add-note "<blocker details>"`
