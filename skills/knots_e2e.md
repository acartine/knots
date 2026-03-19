---
name: knots-e2e
description: >-
  Use the Knots workflow through `kno` when asked to drive a knot end to end,
  run a claimed knot to completion, or keep advancing a knot until it reaches a
  terminal state such as `SHIPPED` or `DEFERRED`.
---

# Knots E2E

## Workflow

Follow this sequence:

```bash
kno claim <id>
```

- Record the current state from the claim output.
- If the current state is `SHIPPED` or `DEFERRED`, stop cleanly.
- Use the claim output to determine the current state's completion goals.
- Do the work and validate it.
- If the goals were met, advance with a guarded state check:

```bash
kno next <id> --expected-state <current_state>
```

- Record the new current state from the `kno next` output.
- Repeat the work/validate/advance loop until the current state is `SHIPPED` or
  `DEFERRED`.
- If you are blocked, validation fails, or the state's goals were not met,
  roll back safely and stop:

```bash
kno rollback <id>
```

Do not invent alternate transition workflows. Prefer `claim`, `next`, and
`rollback` over manual state mutation unless the user explicitly asks for it.
Do not use `kno show` as the primary control-flow source when `claim`/`next`
already provide the state needed to continue safely.

## Session close behavior

- In an interactive session, briefly say what changed and the final knot state.
- In a non-interactive session, stop cleanly after the knot workflow is complete.
