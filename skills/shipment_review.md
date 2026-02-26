# Shipment Review

## Input
- Knot in `ready_for_shipment_review` state
- Code merged to main, CI green

## Actions
1. Verify the change is live on main branch
2. Confirm CI/CD pipeline completed successfully
3. Validate no regressions in dependent systems
4. Final sign-off

## Output
- Approved: `kno state <id> shipped`
- Issues found: `kno state <id> ready_for_shipment` to retry
- Critical regression: `kno state <id> ready_for_implementation` to fix

## Failure Modes
- Deployment issue: `kno state <id> ready_for_shipment` with details
- Regression detected: `kno state <id> ready_for_implementation` with bug report
