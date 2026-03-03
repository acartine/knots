# Shipment Review

## Input
- Knot in `ready_for_shipment_review` state
- Code merged to main, CI green

## Actions
1. Verify the change is live on main branch
2. Confirm every commit from implementation/shipment is tagged on the knot:
   - Use the `commit:` prefix for each tag.
   - Each tag must include a short hash from
     `git rev-parse --short=12 <commit>` (not the full 40-character hash).
3. Confirm CI/CD pipeline completed successfully
4. Validate no regressions in dependent systems
5. Final sign-off

## Output
- Approved:
  `kno next <id> <currentState> --actor-kind agent --agent-name <AGENT_NAME>`
  `--agent-model <AGENT_MODEL> --agent-version <AGENT_VERSION>`
- Needs revision: `kno update <id> --status ready_for_implementation --add-note "<blocker details>"`
- Critical regression:
  `kno update <id> --status ready_for_implementation --add-note "<blocker details>"`

## Failure Modes
- Deployment issue: `kno update <id> --status ready_for_shipment --add-note "<blocker details>"`
- Regression detected:
  `kno update <id> --status ready_for_implementation --add-note "<blocker details>"`
