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
6. Create a hierarchy of knots via `kno new "<title>"` for parent knots, `kno q "title"` for child knots and `kno edge <id> parent_of <id>` for edges

## Output
- Detailed implementation plan attached as a knot note
- Hierarchy of knots created
- Transition: `kno next <id>`

## Failure Modes
- Insufficient context: `kno update <id> --status ready_for_planning --add-note "<note>"`
- Out of scope / too complex: `kno update <id> --status ready_for_planning --add-note "<note>"`
