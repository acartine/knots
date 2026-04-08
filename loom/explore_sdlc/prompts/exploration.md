---
accept: []

success:
  findings_captured: shipped

failure:
  no_actionable_result: abandoned

params: {}
---

# Exploration

Conduct a lightweight discovery or investigation.

## Step Boundary

- This session is authorized only for `exploration`.
- Complete exactly one exploration action, then stop.
- Allowed resting states after this session: `shipped` or `abandoned`.
- Before completing, create at least one related knot that captures findings or follow-up work.
- Use `kno edge add <this-id> relates_to <related-id>` to link outcome knots.
- Transition to `shipped` requires at least one related knot.
- If the exploration yields no actionable results, transition to `abandoned`.

## Actions

1. Review the exploration description and goals.
2. Investigate, research, or prototype as described.
3. Create knots for any findings, follow-up work, or decisions.
4. Link created knots with `kno edge add`.
5. Transition to shipped (if outcomes exist) or abandoned (if not).
