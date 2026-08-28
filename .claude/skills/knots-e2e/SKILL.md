---
name: knots-e2e
description: >-
  Use the Knots workflow through `kno` when asked to drive a knot end to end,
  run a claimed knot to completion, or keep advancing a knot until it reaches a
  terminal state such as `SHIPPED`, or a passive waiting state such as
  `BLOCKED` or `DEFERRED`.
---

# Knots E2E

## Agent identity

Create or receive a lease before the first claim:

```bash
kno lease create --nickname "<session-name>"
```

Use the current claim's lease for that claim's advance, notes, and
handoff-capsule commands. Agent identity for notes, handoff capsules, state
transitions, and gate decisions comes from the bound lease.

Do not copy legacy `--*-agentname/model/version` identity flags from telemetry
or command history. They are deprecated, ignored by current `kno`, and can
leave metadata attributed as `[unknown <date>]` when no lease is bound.

## Lease lifecycle

For ordinary CLI work, a lease lasts for one claimed action. The initial
`kno claim --e2e <id> --lease <lease-id>` activates and binds the lease. A
successful `kno next ... --lease <lease-id>` releases that claim: Knots
unbinds the lease and marks it `lease_terminated`. `kno rollback` releases the
lease the same way.

After every successful `kno next`, treat the lease id you just used as spent.
Before the next e2e claim, create or receive a fresh lease and pass that new
lease id to `kno claim --e2e`. Do not try to reuse or extend the old lease.

## Dead-lease recovery

Treat a lease as dead when `kno` reports it terminated or expired. Never reuse
or extend it. If recovery leaves the knot in a queue state (`ready_for_*`),
create a fresh lease and re-claim it in e2e mode:

```bash
kno lease create --nickname "<session-name>"
kno claim --e2e <id> --lease <new-lease-id>
```

Use the new claim output and resume the e2e loop only after that claim succeeds.
If `kno claim` reports that the knot is in an action state held by an active
lease, stop: another actor owns the action. Do not roll it back, rebind it, or
use `kno state --force`.

## Invocation precedence

When this skill is invoked, the agent MUST claim with the `--e2e` flag so the
workflow boundary surfaced by `kno` advertises e2e-continuation semantics
rather than the default single-action boundary:

```bash
kno claim --e2e <id> --lease <lease-id>
```

The claim output will then carry `kind: e2e_continuation` in the workflow
boundary section (and `"workflow_boundary_kind": "e2e_continuation"` plus
`"e2e": true` in `--json` output). That signal authorizes the agent to keep
working past the per-claim "complete exactly one workflow action" boundary
that ordinary claims emit. After every successful `kno next`, create or
receive a fresh lease, immediately re-claim with
`kno claim --e2e <id> --lease <new-lease-id>`, and continue until the knot
reaches `SHIPPED`, `BLOCKED`, or `DEFERRED`.

If the claim output shows `kind: single_action` (i.e. `--e2e` was not
passed), do NOT continue past the boundary. Stop after the single action
and ask the user to re-invoke with the e2e skill if they want to continue.

## Workflow

Follow this sequence:

```bash
kno claim --e2e <id> --lease <lease-id>
```

- If you are working inside a git worktree, run Knots commands as
  `kno -C <path_to_repo> ...` because Knots is installed for the repo root,
  not the worktree path.
- Record the current state from the claim output.
- If the current state is `SHIPPED`, `BLOCKED`, or `DEFERRED`, stop cleanly.
- Use the claim output to determine the current state's completion goals.
- Do the work and validate it.
- If the workflow prompt asks for handoff context between action states, attach
  it under the bound lease before advancing:

```bash
kno update <id> -H "<capsule>" --lease <lease-id>
```

- If the goals were met, advance with a guarded state check:

```bash
kno next <id> --expected-state <current_state> --lease <lease-id>
```

- Record the new current state from the `kno next` output.
- Treat the lease used for `kno next` as spent; ordinary CLI leases are now
  unbound and `lease_terminated`.
- Create or receive a fresh lease for the next claim.
- Re-claim immediately, again with `--e2e`, to enter the next action state:

```bash
kno claim --e2e <id> --lease <new-lease-id>
```

- Repeat the claim/work/validate/advance loop until the current state is
  `SHIPPED`, `BLOCKED`, or `DEFERRED`.
- If you are blocked, validation fails, or the state's goals were not met,
  roll back safely and stop:

```bash
kno rollback <id> --lease <lease-id>
```

When the action prompt declares a named failure outcome for what went wrong,
record that outcome instead. It moves the knot to the outcome's declared
target state — which may skip past the immediately preceding queue state, as
`merge_conflicts` does — and terminates and unbinds the lease in the same
atomic operation:

```bash
kno rollback <id> --outcome <outcome-name> --lease <lease-id>
```

The prompt's Failure Modes section lists the outcome names it accepts; an
undeclared name is rejected without changing state or lease.

If the claimed knot lists children, handle the children first:
- Claim each child knot with `--e2e` and its own fresh lease, then follow that
  child prompt to completion.
- When the child knots are handled, evaluate the outcomes.
- If every child advanced, advance the parent and continue the loop.
- If any child rolled back, roll the parent back and stop.

Do not invent alternate transition workflows. Prefer `claim`, `next`, and
`rollback` over manual state mutation unless the user explicitly asks for it.
Do not use `kno show` as the primary control-flow source when `claim`/`next`
already provide the state needed to continue safely.

## Why `--e2e` exists

Ordinary `kno claim <id>` emits a workflow boundary instructing the agent to
"complete exactly one workflow action, then stop." That boundary is correct
for one-shot claims and prevents agents from over-running their authorization.
When the user has explicitly asked for end-to-end execution, however, the
default boundary fights the skill. Passing `--e2e` switches the boundary to
`e2e_continuation`, which:

1. Carries an unambiguous, machine-readable signal (`workflow_boundary_kind`)
   so agents can detect e2e mode without inferring intent from prose.
2. Tells the agent to re-claim after `kno next` and continue across action
   states.
3. Authorizes terminal-state movement for this run, since `SHIPPED` /
   `BLOCKED` / `DEFERRED` are valid stopping points for an e2e run.

Ordinary one-action claims continue to use the `single_action` boundary;
`--e2e` is opt-in.

## Session close behavior

- In an interactive session, briefly say what changed and the final knot state.
- In a non-interactive session, stop cleanly after the knot workflow is complete.
