# Planning

## Input
- Knot in `ready_for_planning` state
- Knot title, description, and any existing notes/context

## Invariant Adherence
- If the knot has invariants, read and understand each one before planning.
- Every step in the plan must respect all invariant conditions.
- Scope invariants constrain what the work may touch.
- State invariants constrain what must remain true throughout execution.
- If any planned step would violate an invariant, redesign the approach or
  flag the conflict in the plan note.

## Actions
1. Analyze the knot requirements and constraints
2. Review knot invariants and ensure the plan respects them
3. Research relevant code, dependencies, and prior art
4. Draft an implementation plan with steps, file changes, and test strategy
5. Estimate complexity and identify risks
6. Write the plan as a knot note via `kno update <id> --add-note "<plan>"`
7. Create a hierarchy of knots via `kno new "<title>"` for parent knots,
   `kno q "title"` for child knots and `kno edge <id> parent_of <id>`
   for edges

## Output
- Detailed implementation plan attached as a knot note
- Hierarchy of knots created
- Add a handoff capsule summarizing the plan:
  `kno update <id> --add-handoff-capsule "<handoff_capsule>"`
  `--handoff-username <username> --handoff-datetime <date RFC3339>`
  `--handoff-agentname <agentname> --handoff-model <model>`
  `--handoff-version <model_version>`
- Transition:
  `kno next <id> <currentState> --actor-kind agent --agent-name <AGENT_NAME>`
  `--agent-model <AGENT_MODEL> --agent-version <AGENT_VERSION>`

## Failure Modes
- Insufficient context:
  `kno update <id> --status ready_for_planning --add-note "<note>"`
  `kno update <id> --add-handoff-capsule "<reason for deferral>"`
  `--handoff-username <username> --handoff-datetime <date RFC3339>`
  `--handoff-agentname <agentname> --handoff-model <model>`
  `--handoff-version <model_version>`
- Out of scope / too complex:
  `kno update <id> --status ready_for_planning --add-note "<note>"`
  `kno update <id> --add-handoff-capsule "<reason out of scope>"`
  `--handoff-username <username> --handoff-datetime <date RFC3339>`
  `--handoff-agentname <agentname> --handoff-model <model>`
  `--handoff-version <model_version>`
