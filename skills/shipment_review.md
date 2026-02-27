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
- Approved: `kno next <id>`
- Needs revision: `kno update <id> --status ready_for_implementation --add-note "<blocker details>"`
- Critical regression: `kno update <id> --status ready_for_implementation --add-note "<blocker details>"`

## Failure Modes
- Deployment issue: `kno update <id> --status ready_for_shipment --add-note "<blocker details>"`
- Regression detected: `kno update <id> --status ready_for_implementation --add-note "<blocker details>"`
