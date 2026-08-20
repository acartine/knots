# Leases

A **lease** is a session token that is automatically created when an agent claims
a knot and terminated when the agent advances to the next step. Leases exist to
answer a simple question: _is anyone actively working on this right now?_

Every claim gets its own lease. There is no sharing or reuse across claims.

## Why leases exist

- **Concurrency safety** -- the *pull* half of sync is blocked while a lease is
  active, so a remote update cannot move a knot out from under an agent
  mid-action. Push is never blocked (see below).
- **Session auditing** -- leases record which agent (name, model, version) held a
  claim, giving a durable audit trail of who did what.
- **Stuck-work detection** -- leases expire after a configurable timeout. Expired
  leases unblock pull automatically and are cleaned up on the next interaction.

## Push is never blocked

`kno push` publishes local event files and runs regardless of leases. Events
are immutable and named `events/YYYY/MM/DD/{uuidv7}-{event_type}.json`, so two
machines can never write different bytes to the same path, and every event
being published is already durable in the local store. Publishing mid-claim
therefore exports nothing the local store does not already hold -- it just
gives other machines visibility into work that is in flight.

Only the pull half is held back, because pull imports remote index events into
the hot cache and can change a knot an agent is actively working on.

`kno sync` reflects that split. With a lease active it publishes and then
defers only the pull:

```
$ kno sync
sync push(local_event_files=12 copied_files=3 committed=true pushed=true) \
    pull deferred: 1 active lease(s); pull will run when leases are terminated
```

The JSON form carries the same detail, so automated callers can tell "pushed,
pull deferred" apart from "nothing happened":

```json
{
  "status": "deferred",
  "active_leases": 1,
  "push": { "local_event_files": 12, "copied_files": 3,
            "committed": true, "pushed": true, "commit": "9f2c..." }
}
```

`kno pull` on its own still refuses while a lease is active.

## Lifecycle

Each lease moves through three states. Expiry can terminate a lease from either
`lease_ready` or `lease_active`:

```mermaid
stateDiagram-v2
    [*] --> lease_ready : kno lease create
    lease_ready --> lease_active : kno claim (lease activated)
    lease_active --> lease_terminated : kno next (work advanced)
    lease_active --> lease_terminated : kno rollback (work rewound)
    lease_active --> lease_terminated : kno rollback --outcome (action failed)
    lease_active --> lease_terminated : kno lease terminate
    lease_active --> lease_terminated : timeout expired
    lease_ready --> lease_terminated : timeout expired
    lease_active --> lease_terminated : kno doctor --fix
    lease_terminated --> [*]
```

### States

| State | Meaning |
|-------|---------|
| `lease_ready` | Created; waiting to be activated via claim |
| `lease_active` | Claim is live; the pull half of sync is blocked; work is in progress |
| `lease_terminated` | Completed, expired, or abandoned; pull is unblocked |

## Timeout and expiry

Every lease has an expiry timestamp set at creation. The default timeout is
**600 seconds (10 minutes)**, configurable via `--timeout-seconds`:

```bash
kno lease create --nickname "session" --timeout-seconds 900   # 15 min
kno claim <id> --lease <lease-id> --timeout-seconds 1800      # 30 min
```

### Heartbeat

Any write command that touches a bound knot (`kno update`) automatically
refreshes the lease expiry. Agents that are actively working don't need to
explicitly extend -- each interaction resets the timer.

### Explicit extension

Agents that need more time without touching the knot can extend directly:

```bash
kno lease extend --lease-id <id>                     # reset to 10 min
kno lease extend --lease-id <id> --timeout-seconds 1800  # 30 min
```

### Lazy materialization

Expired leases are not terminated by a background timer. Instead, expiry is
checked whenever any code path reads a lease. When an expired lease is
detected:

1. The lease state is written to `lease_terminated` in the database.
2. The lease is unbound from the work knot.
3. The knot is rolled back to its previous queue state.
4. The pull half of sync is unblocked.

This means an expired lease may briefly appear active in the database until
the next interaction triggers materialization.

### Requeueing dead claims

An expired or explicitly terminated lease can be encountered while still bound
to a knot in an action state. Knots recovers that dead claim by unbinding the
lease and returning the knot to its preceding queue state, where a worker can
claim it with a fresh lease. A terminated lease is never reusable or extendable.

An action state held by another active lease is not a recovery target. A later
worker must stop rather than roll it back, rebind it, or use `kno state --force`.

## One lease per claim

Each `kno claim` call creates and activates a dedicated lease for that knot.
Leases are never shared between claims. When the claim completes via
`kno next`, the lease is terminated and the binding is removed. A claim that
ends in failure releases the lease the same way: `kno rollback` rewinds to the
preceding queue state, and `kno rollback --outcome <name>` records a
workflow-declared failure outcome and moves to that outcome's target state.
Both terminate the lease and unbind it in the same operation, so a failed
action never leaves a knot claimed.

```mermaid
sequenceDiagram
    participant Agent
    participant Knots

    Agent->>Knots: kno claim --timeout-seconds 600
    Knots->>Knots: create lease (lease_ready, expiry=now+600)
    Knots->>Knots: activate lease (lease_active)
    Knots->>Knots: bind lease to work knot
    Knots-->>Agent: work knot + lease id

    Note over Agent,Knots: agent performs work...
    Agent->>Knots: kno update (heartbeat refreshes expiry)

    Agent->>Knots: kno next --lease <id>
    Knots->>Knots: terminate lease (lease_terminated)
    Knots->>Knots: unbind lease from work knot
    Knots-->>Agent: next knot (or done)
```

## Graceful completion with expired leases

If an agent calls `kno next` with an expired lease **and** nobody else has
claimed the knot, the progression is allowed. This prevents wasting work when
the only issue is a timeout -- the agent finished, just a bit late.

If another agent has already reclaimed the knot, the original agent's `kno next`
will fail with an instructive error.

## Lease identifiers

Lease IDs are returned at claim time for use with `--lease` on `kno next` and
`kno lease extend`. Standard output (`kno show`, `kno ls`) does not include
lease identifiers to prevent external callers from hijacking sessions.

You can inspect leases directly with `kno lease ls` and `kno lease show <id>`.

## Lease ownership

Lease knots replicate like any other knot, so a lease created on one machine can
land in another machine's cache through a normal pull. Every lease therefore
records the machine and process that created it:

```
$ kno lease show <id>
  lease_owner:  machine=a1fe982a5844ddd1c07e25b9fd9653c1 pid=3898159
```

The machine id is an opaque digest stored at `.knots/machine-id`. It is local to
the machine (never synced), stable across processes, and carries no hostname or
username. Set `KNOTS_MACHINE_ID` to pin it explicitly.

Ownership decides which leases block work here:

- `kno lease ls` lists the active leases held by **this** machine. Use
  `kno lease ls --all` to see every lease, each labelled with its owner.
- The pull half of sync is blocked only by leases this machine holds. A lease
  pulled from another machine never blocks your pull, and nothing blocks your
  push.
- A lease with no recorded owner comes from an event written before ownership
  tracking existed. Those are treated as local, so existing stores keep their
  previous, conservative behavior.

## Stuck lease recovery

```bash
kno doctor          # report stuck/expired leases
kno doctor --fix    # terminate all, unbind knots, unblock pull
```

## Manual lease management

In most workflows leases are fully automatic. Manual commands exist for
inspection, debugging, and recovery:

```bash
# Inspect
kno lease ls                  # active leases only
kno lease ls --all            # include terminated
kno lease ls --json
kno lease show <id>
kno lease show <id> --json

# Create an external lease (advanced)
kno lease create --nickname "my-session" --type agent \
    --agent-name claude --model opus --model-version 4.6 \
    --timeout-seconds 1800
kno lease create --nickname "manual-fix" --type manual

# Extend
kno lease extend --lease-id <id>
kno lease extend --lease-id <id> --timeout-seconds 1800

# Terminate
kno lease terminate <id>
```

An external lease can be passed to a claim via `--lease <id>`, which activates
the pre-created lease instead of creating a new one:

```bash
kno claim <knot-id> --lease <lease-id> --timeout-seconds 900
```

## Agent identity propagation

The lease is the **declared source of agent identity** (name, model,
version, provider, agent type) on a claimed knot. Identity is supplied
exactly once, at `kno lease create`, and travels with the lease for the
rest of the claim. Knots reads the bound lease's `agent_info` and stamps
it onto every record produced while the lease is active.

### The rule

`kno lease create` is the **only** subcommand that accepts agent identity
flags. On every other subcommand, the agent-identity flags are
**deprecated and ignored at runtime**. Specifically:

- `--agent-name`, `--agent-model`, `--agent-version` on `kno claim`,
  `kno poll --claim`, `kno next`, `kno rollback`, `kno gate evaluate`,
  and `kno step annotate`.
- `--note-agentname`, `--note-model`, `--note-version` on
  `kno update --add-note`.
- `--handoff-agentname`, `--handoff-model`, `--handoff-version` on
  `kno update --add-handoff-capsule`.

These flags are still **accepted syntactically** so legacy callers do not
break. Their values are **discarded**; identity comes from the bound
lease's `agent_info`. A future release will reject the flags outright.

`--actor-kind` (`agent` vs `human`) is orthogonal to agent identity and is
unaffected by this rule. Continue to pass `--actor-kind` where the CLI
accepts it.

### Deprecation warning

Whenever a deprecated agent-identity flag is supplied on a non-lease-create
subcommand, Knots writes a three-line warning to stderr:

1. The flag is deprecated and will be rejected in a future release.
2. Its value is ignored on this subcommand — identity is read from the
   bound lease.
3. If no lease is bound, create one via
   `kno lease create --agent-name ... --model ... --model-version ...` and
   pass `--lease <id>` on claim.

The warning is informational: the command still runs. The offending flag
has no effect on the record Knots writes.

### Contract for a Knots Client

A *Knots Client* is any automated system driving `kno` on behalf of an
agent (as opposed to a human at an interactive shell). A Knots Client
declares identity by creating a lease:

```bash
lease_id=$(kno lease create --nickname "my-session" --type agent \
    --agent-name <name> --model <model> --model-version <version> \
    --provider <provider> --agent-type <type> --json | jq -r .id)
kno claim <knot-id> --lease "$lease_id"
```

All subsequent `kno` calls within that claim operate on the bound lease
and MUST NOT pass agent-identity flags. The deprecation warning above is
the feedback loop for clients that still do.

### What Knots stamps from the lease

When a knot has a bound lease, Knots reads the lease's `agent_info` and
stamps it onto records produced by the active claim:

| Record | Fields stamped | Trigger |
|---|---|---|
| Note | `agentname`, `model`, `version`, `username` | `kno update --add-note` |
| Handoff capsule | `agentname`, `model`, `version`, `username` | `kno update --add-handoff-capsule` |
| Step-history entry (claim/transition) | `agent_name`, `agent_model`, `agent_version` | `kno claim`, `kno poll --claim`, `kno next` |
| Step-history entry (rollback) | `agent_name`, `agent_model`, `agent_version` | `kno rollback`, `kno rollback --outcome` |
| Gate decision metadata | `agentname`, `model`, `version` | `kno gate evaluate` |

Lookup is performed by `resolve_lease_agent_info` in
[`src/write_dispatch/helpers.rs`](../src/write_dispatch/helpers.rs): given a
knot id, Knots reads the knot's currently bound `lease_id`, loads the
lease, and returns its `agent_info`. If no lease is bound, these fields
stay unset on the record.

```mermaid
flowchart LR
    L[Bound lease<br/>lease.agent_info]
    L -->|"resolve_lease_agent_info"| S[Step-history entry<br/>agent_name / agent_model / agent_version]
    L -->|"resolve_lease_agent_info"| N[Note<br/>agentname / model / version]
    L -->|"resolve_lease_agent_info"| H[Handoff capsule<br/>agentname / model / version]
    L -->|"resolve_lease_agent_info"| G[Gate decision<br/>agentname / model / version]
```

Identity travels with the artifact for the rest of its life. Once stamped,
the fields are part of the record and don't need to be looked up again.

### Contract for clients

Clients (Foolery, scripts, future automation) MUST treat the lease as
authoritative for the duration of the claim:

1. **Declare once.** Pass the agent identity to `kno lease create` and only
   to `kno lease create`.
2. **Refer by id.** Pass `--lease <id>` on `kno claim` / `kno poll --claim`
   / `kno next` / `kno lease extend`. Never pass `--agent-*` flags on
   those subcommands.
3. **Read, don't re-derive.** When a client needs to display "what agent
   is on this knot right now," it reads the lease (`kno lease show
   <id> --json`) or the knot's most recent step / note / handoff capsule.
   It does not re-parse a model string from the environment, the agent's
   binary version, or any other ambient state.
4. **No fallbacks.** If `resolve_lease_agent_info` returns `None` for a
   record, the artifact's agent fields are left unset. Clients must not
   substitute literals like `"claude"` or `"unknown"`; the absence of
   identity is itself information.

Foolery's client-side companion contract is at
[`foolery/docs/knots-agent-identity-contract.md`](https://github.com/acartine/foolery/blob/main/docs/knots-agent-identity-contract.md).
The two documents must agree.

## Known limitations

- **Expired lease + reclaimed knot = wasted work.** If a lease expires and
  another agent claims the knot before the original agent finishes, the
  original agent's in-flight work is lost. This is an accepted tradeoff;
  choosing a longer timeout or using heartbeat-producing commands mitigates it.
