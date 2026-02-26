# Planning

## Input
- Knot in `ready_for_planning` state
- Knot title, description, and any existing notes/context

## Actions
1. Analyze the knot requirements and constraints
2. Research relevant code, dependencies, and prior art
3. Draft an implementation plan with steps, file changes, and test strategy
4. Estimate complexity and identify risks
5. Write the plan as a knot note via `kno update <id> --add-note "<plan>"`

## Output
- Detailed implementation plan attached as a knot note
- Transition: `kno state <id> ready_for_plan_review`

## Failure Modes
- Insufficient context: `kno state <id> deferred` with note explaining gaps
- Out of scope / too complex: `kno state <id> abandoned` with rationale
