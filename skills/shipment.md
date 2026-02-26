# Shipment

## Input
- Knot in `ready_for_shipment` state
- Approved implementation on feature branch

## Actions
1. Merge feature branch to main
2. Run `make sanity` on merged main
3. Push main to remote
4. Verify CI passes on remote

## Output
- Code merged and pushed to main
- CI green on remote
- Transition: `kno state <id> ready_for_shipment_review`

## Failure Modes
- Merge conflicts: `kno state <id> ready_for_implementation` to resolve
- CI failure after merge: `kno state <id> ready_for_implementation` to fix
