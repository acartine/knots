# Implementation

## Input
- Knot in `ready_for_implementation` state
- Approved implementation plan (in knot notes)

## Actions
1. Create a feature branch from main
2. Implement changes following the plan
3. Write tests for all new behavior
4. Run `make sanity` (fmt + clippy + test + coverage)
5. Commit and push the feature branch

## Output
- Working implementation on feature branch
- All tests passing with coverage threshold met
- Transition: `kno state <id> ready_for_implementation_review`

## Failure Modes
- Blocked by dependency: `kno state <id> deferred` with blocker details
- Implementation infeasible: `kno state <id> ready_for_planning` for replanning
