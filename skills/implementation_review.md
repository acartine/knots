# Implementation Review

## Input
- Knot in `ready_for_implementation_review` state
- Feature branch with implementation

## Actions
1. Review code changes for correctness and style
2. Verify tests cover the requirements
3. Check `make sanity` passes
4. Validate no security issues or regressions introduced
5. Approve or request changes

## Output
- Approved: `kno state <id> ready_for_shipment`
- Needs changes: `kno state <id> ready_for_implementation` with feedback

## Failure Modes
- Critical issues found: `kno state <id> ready_for_implementation` with details
- Architecture concern: `kno state <id> ready_for_planning` for replanning
