# Knots Taxonomy

> Shared vocabulary for this codebase. Humans: edit freely — your definitions win.
> Agents: read before writing code. Update via `/taxonomize` (preserves human edits).

Last auto-run: 2026-04-16 · Scope: full repo

## How to read this file

- **Nouns** are domain entities and concepts.
- **Verbs** are operations performed on nouns.
- **Phrases** are compound terms that carry more meaning than their parts.
- **States** are an explicit sub-section because the state vocabulary is the backbone of the workflow.
- Citations like `path/file.ext:42` anchor each term to real usage.
- `<!-- human -->` marks hand-written entries; `<!-- auto -->` marks generated ones.
- ⚠ markers flag things to review: `overloaded`, `ambiguous`, `stale`, `divergence`.

---

## Nouns

### action state <!-- auto -->
A workflow state where something is actively being worked (e.g. `planning`, `implementation`, `shipment`). Contrast with *queue state*.
- `README.md:32`
- Related: *queue state*, *terminal state*, *passive escape state*.

### archival / cold sweep <!-- auto -->
Background process that moves knots in terminal states into the cold catalog and out of the hot cache.
- `src/app/archival.rs:69` — `run_cold_sweep`
- Related: *cold knot*, *cold catalog*.

### cache lock <!-- auto -->
Exclusive advisory lock serializing SQLite writes. Must be acquired *after* the repo lock (never reverse).
- `knots-architecture-v4.md:402`
- `src/locks.rs` (lock ordering invariant)

### child knot <!-- auto -->
A knot reached from a parent via a `parent_of` edge. Parent terminal transitions can cascade to children.
- `src/app/types.rs:69` — `ChildSummary`
- Related: *edge*, *parent_of*, *cascade*.

### cold catalog <!-- auto -->
SQLite table of knots in terminal states stored as id/title/state summaries. Not synced by default; rehydrated on demand.
- `src/db/catalog.rs:55` — `upsert_cold_catalog`
- Related: *cold knot*, *warm catalog*, *materialized cache*.

### cold knot <!-- auto -->
A knot in a terminal state (`shipped`, `abandoned`). Not kept in the hot cache; lives in the cold catalog.
- `knots-architecture-v4.md:291`
- `src/app/types.rs:92` — `ColdKnotView`

### ColdSweepReport <!-- auto -->
Summary of knots transitioned to cold during an archival sweep.
- `src/app/archival.rs:34`

### dead letter <!-- auto -->
A knot that cannot proceed — typically abandoned without upstream resolution. Informal term from the architecture doc.
- `knots-architecture-v4.md` (terminal/cold semantics)

### distribution mode <!-- auto -->
How a project's knot data is distributed: `Git` (synced via a git branch) or `LocalOnly` (no remote).
- `src/project.rs:11` — `DistributionMode`

### doctor check <!-- auto -->
A named health probe run by `kno doctor`. Current checks: `lock_health`, `worktree`, `remote`, `hooks`, `version`, `schema_version`, `stuck_leases`, `terminal_parents`, `cold_tier_imbalance`.
- `src/doctor.rs:173` (and neighbors)

### edge <!-- auto -->
A directed relationship from one knot to another, labeled by kind. Known kinds: `blocked_by`, `blocks`, `parent_of`, `relates_to`.
- `src/db/catalog.rs:223` — `EdgeRecord`
- `src/app/types.rs:62` — `EdgeView`
- `src/list_layout.rs:154` (kind matching)

### event <!-- auto -->
An immutable, append-only record capturing one change or activity on a knot. Events are the source of truth; SQLite is a materialized cache.
- `src/events/mod.rs:201` — `EventRecord`
- `knots-architecture-v4.md:90`

### event file <!-- auto -->
A single JSON file holding one event, named by UUIDv7 plus event type, under `.knots/events/YYYY/MM/DD/` or `.knots/index/YYYY/MM/DD/`.
- `src/events/mod.rs:280` — `relative_path_for_event`
- `knots-architecture-v4.md:112`

### event kind <!-- auto -->
The discriminator on an event. Full-stream kinds live in `FullEventKind` (22 variants, e.g. `KnotCreated`, `KnotStateSet`, `KnotEdgeAdd`, `KnotLeaseIdSet`, `KnotReviewDecision`). The index stream has only `KnotHead`.
- `src/events/mod.rs:34` — `FullEventKind`
- `src/events/mod.rs:89` — `IndexEventKind`

### event stream <!-- auto -->
A logical partition of events: `Full` (complete payloads) or `Index` (head summaries).
- `src/events/mod.rs:18` — `EventStream`

### EventWriter <!-- auto -->
Facade that serializes and writes `EventRecord`s to disk under the store root.
- `src/events/mod.rs:245`

### execution plan <!-- auto -->
A coordination artifact attached to a knot that tracks how a set of other knots should be executed, organized into waves and steps.
- `docs/execution-plans.md:5`
- `src/domain/execution_plan.rs:50` — `ExecutionPlanData`

### execution plan agent <!-- auto -->
A role assignment within an execution plan (role, count, specialty).
- `src/domain/execution_plan.rs:4` — `ExecutionPlanAgent`

### execution plan step <!-- auto -->
An ordered checkpoint inside a wave. Steps within a wave run sequentially.
- `docs/execution-plans.md:19`
- `src/domain/execution_plan.rs:22` — `ExecutionPlanStep`

### execution plan wave <!-- auto -->
A top-level phase in an execution plan. Waves run sequentially; steps within a wave also run sequentially.
- `docs/execution-plans.md:18`
- `src/domain/execution_plan.rs:32` — `ExecutionPlanWave`

### external edge <!-- auto -->
A visibility-only edge across different stores in a portfolio. Non-authoritative.
- `docs/portfolio-plan.md:206`

### full event <!-- auto -->
A large, complete event payload (state change plus all context) stored in `.knots/events/`. Contrast with *index event*.
- `src/events/mod.rs:108` — `FullEvent`

### gate <!-- auto -->
A knot of type `Gate`: a coordination barrier whose evaluation (`Yes` / `No`) can reopen target knots.
- `src/domain/gate.rs:60` — `GateData`
- `src/app/gate.rs:14` — `evaluate_gate`

### gate decision <!-- auto -->
The outcome of evaluating a gate: `Yes` (pass) or `No` (fail, triggering reopens).
- `src/app/types.rs:86` — `GateDecision`

### gate mode <!-- auto -->
How a profile treats a gate: `Required`, `Optional`, or `Skipped`.
- `src/profile.rs:17` — `GateMode`

### gate owner kind <!-- auto -->
Who is authorized to decide a gate: `Human` or `Agent`.
- `src/domain/gate.rs:9` — `GateOwnerKind`

### handoff capsule <!-- auto -->
Structured metadata added as a note to pass context from one workflow step to the next actor.
- `README.md:402`
- `src/events/mod.rs:34` — `KnotHandoffCapsuleAdded`

### hot cache <!-- auto -->
Fully-rehydrated SQLite rows for knots updated within the hot window. Serves the default warm path.
- `knots-architecture-v4.md:266`
- Related: *hot window*, *warm knot*.

### hot knot <!-- auto -->
A knot currently in the hot cache: all fields indexed and queryable.
- `knots-architecture-v4.md:266`

### hot window <!-- auto -->
Configurable retention window (default 7 days) for keeping knots in the hot cache; controlled by the `hot_window_days` meta key.
- `src/db/migrations.rs:380`
- `knots-architecture-v4.md:287`

### index event <!-- auto -->
A lightweight head-summary event (just enough for listings and sync) stored in `.knots/index/`.
- `src/events/mod.rs:156` — `IndexEvent`
- Only kind today: `KnotHead` (`idx.knot_head`).

### invariant <!-- auto -->
A named constraint asserted on a knot. `type` is `Scope` or `State`; `condition` holds the predicate text. Gates map invariants to reopen targets on failure.
- `src/domain/invariant.rs:59`
- `src/domain/invariant.rs:8` — `InvariantType`

### knot <!-- auto -->
The foundational unit this product tracks: a named piece of work, gate, lease, exploration, or execution plan. Everything else decorates it.
- `README.md:24`
- `src/domain/knot_type.rs:8` — `KnotType`

### knot head <!-- auto -->
The minimal snapshot of a knot (`id`, `title`, `state`, `updated_at`) carried by an `IndexEvent`. Enables fast listing and sync without full-event fetch.
- `knots-architecture-v4.md:148`
- `src/events/mod.rs:89` — `IndexEventKind::KnotHead`

### knot id <!-- auto -->
The primary identifier for a knot. Must be sortable — the project uses UUIDv7; ULID is accepted as an alternative.
- `src/knot_id.rs`
- `knots-architecture-v4.md:127`

### knot type <!-- auto -->
The category of a knot: `Work` (default), `Gate`, `Lease`, `Explore`, `ExecutionPlan`.
- `src/domain/knot_type.rs:8` — `KnotType`

### KnotCacheRecord <!-- auto -->
Hot-cache SQLite row with all queryable fields of a knot.
- `src/db.rs:120`

### KnotView <!-- auto -->
Complete serializable projection of a knot with all properties, relationships, metadata, and history. Returned by most App read operations.
- `src/app/types.rs:16`

### lease <!-- auto -->
A session token created when an agent claims a knot. While active and held by this machine, it holds back pull for the knots it covers (push is never blocked); terminating it releases those knots for the next pull. Leases themselves are knots (of type `Lease`).
- `docs/leases.md:3`
- `src/lease.rs:6` — `create_lease`
- `src/domain/lease.rs:90` — `LeaseData`

### lease type <!-- auto -->
Who owns a lease: `Agent` (provisioned for an automated worker) or `Manual` (human-held).
- `src/domain/lease.rs:8` — `LeaseType`

### loom bundle <!-- auto -->
A compiled workflow definition under `loom/<name>/dist/bundle.json`, produced from the `.loom` sources in the same directory. Loaded at runtime by the installed-workflows layer.
- `loom/README.md`
- `src/installed_workflows/loom.rs`

### materialized cache <!-- auto -->
The local SQLite database: a materialized view over the event log. Rebuildable at any time from events.
- `knots-architecture-v4.md:13`

### metadata entry <!-- auto -->
A timestamped note or handoff capsule attached to a knot, with author and agent attribution.
- `src/domain/metadata.rs:7` — `MetadataEntry`
- `README.md:256`

### named project <!-- auto -->
A `LocalOnly` project identified by a name (no git backing). Resolved by `resolve_context` when `--project <id>` is used.
- `src/project.rs:73` — `NamedProjectRecord`
- `src/project.rs:200` — `create_named_project`

### passive escape state <!-- auto -->
A non-terminal state where a knot is paused and not claimable. Today: `deferred` (and the edge concept `blocked_by`, not a state).
- `README.md:36`

### portfolio <!-- auto -->
A repo-owned grouping of related Knots projects for discovery and cross-project visibility.
- `docs/portfolio-plan.md:5`

### portfolio member <!-- auto -->
A project record (git-backed repo or named local project) belonging to a portfolio.
- `docs/portfolio-plan.md:40`

### profile <!-- auto -->
A variant of a workflow that assigns ownership and behavior per state (e.g. `autopilot`, `semiauto`). A workflow defines states; a profile decides who does each step.
- `README.md:44`
- `src/profile.rs:67` — `ProfileDefinition`

### profile etag <!-- auto -->
Opaque token representing the current workflow-relevant state of a knot. Attached as a precondition to events for optimistic concurrency.
- `src/events/mod.rs:102` — `WorkflowPrecondition`
- `knots-architecture-v4.md:183` (workflow_etag)

### project context <!-- auto -->
Resolved bundle of repo root, store paths, distribution mode, and project id used by every command.
- `src/project.rs:257` — `resolve_context`
- `src/project.rs:17` — `StorePaths`

### prompt definition <!-- auto -->
An action-state prompt template with parameters, success target, and validation rules; rendered by the managed skills for agents.
- `src/installed_workflows/mod.rs:154` — `PromptDefinition`

### pull drift warning <!-- auto -->
Signal that unpushed local events exceed the configured threshold (`pull_drift_warn_threshold`).
- `src/app/types.rs:100` — `PullDriftWarning`
- `src/db/migrations.rs:428`

### queue state <!-- auto -->
A workflow state where a knot is ready to be picked up (`ready_for_*`). Contrast with *action state*.
- `README.md:33`

### queued write request / response <!-- auto -->
Files under `.knots/writes/` and `.knots/responses/` that serialize CLI write intents through the FIFO write queue.
- `src/write_queue.rs:195` — `QueuedWriteRequest`
- `src/write_queue.rs:207` — `QueuedWriteResponse`

### quarantine (sync) <!-- auto -->
A `knot_sync_quarantine` row recording `(knot_id, base_commit)` for a knot whose events a pull skipped because this machine locally leases it. `base_commit` is the watermark from just before the pass that first skipped the knot, and is set once; it anchors the scoped replay a later pull runs once the knot is no longer locally leased. Distinct from the shared `last_index_head_commit` / `last_full_head_commit` watermark, which advances every pass regardless of what was quarantined.
- `src/db/quarantine.rs`
- `src/sync/apply_quarantine.rs` — `drain_quarantine`

### replication <!-- auto -->
The combined `push + pull` operation. `ReplicationService` owns the push side; `SyncService` owns the pull side. `kno sync` always pushes, then always pulls; the pull half filters out events for locally leased knots instead of deferring wholesale.
- `src/replication.rs` — `ReplicationService`
- `src/replication/summary.rs` — `ReplicationSummary`, `SyncOutcome`

### repo lock <!-- auto -->
Exclusive advisory lock serializing git mutations (fetch, commit, push, ref updates). Taken before the cache lock.
- `knots-architecture-v4.md:398`

### review decision <!-- auto -->
Explicit approve/reject outcome recorded on a knot by a reviewer, with categories and notes.
- `src/events/mod.rs:34` — `KnotReviewDecision`
- `knots-architecture-v4.md:162`

### rework count <!-- auto -->
Count of rejected review decisions accumulated on a knot — a lightweight quality signal.
- `knots-architecture-v4.md:167`

### skill <!-- auto -->
A markdown prompt under `skills/` that describes how an external agent tool (Claude, Codex, OpenCode) should drive the `kno` CLI. Managed skills are installed into host tools by `kno skills install`.
- `skills/README.md`
- `src/managed_skills.rs`

### snapshot <!-- auto -->
An optional append-only rollup of catalogs (active or cold) used to reduce cold-sync cost. Snapshots are advisory; the event log remains authoritative.
- `src/snapshots.rs`
- `knots-architecture-v4.md:116`

### state (knot) <!-- auto -->
The workflow position of a knot; one of 22 `KnotState` variants. See the **States** section below.
- `src/domain/state.rs:6` — `KnotState`

### step history <!-- auto -->
Immutable audit log of state-transition attempts with actor, status, and timestamps.
- `src/domain/step_history.rs:5` — `StepRecord`
- `src/domain/step_history.rs:38` — `StepStatus` (`Started`, `Completed`, `Aborted`, `Failed`)

### step metadata <!-- auto -->
Per-action-state definition in a profile: action name, kind, owner, output, review hint.
- `src/profile.rs:31` — `StepMetadata`
- `README.md:256`

### store paths <!-- auto -->
Computed on-disk paths that make up a project's knot store (db file, events dir, worktree dir, queue dirs).
- `src/project.rs:17` — `StorePaths`

### sync outcome <!-- auto -->
High-level result of `kno sync`: `Completed(ReplicationSummary)`. Pull no longer defers wholesale on a locally held lease; `ReplicationSummary.pull.held_back_knots` names any knots this machine's active leases held back this pass.
- `src/replication/summary.rs` — `SyncOutcome`

### terminal state <!-- auto -->
A knot state that marks completion. Today: `shipped`, `abandoned`, `lease_terminated`. Terminal knots are eligible for the cold tier.
- `src/domain/state.rs:6`
- `knots-architecture-v4.md:291`

### terminal parent resolution <!-- auto -->
Tree of descendants whose state must transition when a parent enters a terminal state.
- `src/state_hierarchy.rs:43` — `TerminalParentResolution`
- `src/state_hierarchy.rs:50` — `TransitionPlan` (`Allowed`, `CascadeTerminal`)

### WAL mode <!-- auto -->
SQLite Write-Ahead Logging mode. Enables snapshot reads during concurrent writes; configured with a 5s busy timeout.
- `README.md:445`
- `ARCHITECTURE.md:53`

### warm catalog / warm knot <!-- auto -->
SQLite table and rows holding only `id + title + state` for knots outside the hot window but not yet cold. Serves fast listings without full rehydration.
- `src/db/catalog.rs:13` — `upsert_knot_warm`
- `knots-architecture-v4.md:269`

### workflow <!-- auto -->
The sequence of states, transitions, gates, and prompts a knot can traverse. Today the built-ins are `work_sdlc`, `gate_sdlc`, `lease_sdlc`, `explore_sdlc`, and `execution_plan_sdlc`.
- `src/installed_workflows/mod.rs:319` — `WorkflowDefinition`
- `loom/README.md`

### workflow ref <!-- auto -->
Normalized reference to a workflow (with optional version) used when binding a knot type to a workflow.
- `src/installed_workflows/knot_type_registry.rs`

### workflow repo config <!-- auto -->
On-disk mapping of knot type → workflow + default profile for the current repo.
- `src/installed_workflows/mod.rs:73` — `WorkflowRepoConfig`

### worktree (knots worktree) <!-- auto -->
Git worktree at `.knots/_worktree/` checked out on the `knots` branch. Where pushes land and pulls apply before cache updates.
- `src/sync/worktree.rs` — `KnotsWorktree`
- `src/project.rs:41`

### write queue <!-- auto -->
FIFO directory-based queue at `.knots/writes/` + `.knots/responses/` that serializes all CLI writes through a single writer process to prevent concurrent modification.
- `src/write_queue.rs`

---

## States

All values come from `src/domain/state.rs:6` (`KnotState`). Workflow column names the built-in workflow(s) where each state appears.

### Work SDLC

| State | Kind | Workflow |
|-------|------|----------|
| `ready_for_planning` | queue | work_sdlc, lease_sdlc, explore_sdlc, execution_plan_sdlc |
| `planning` | action | work_sdlc |
| `ready_for_plan_review` | queue | work_sdlc |
| `plan_review` | action (gate) | work_sdlc |
| `ready_for_implementation` | queue | work_sdlc |
| `implementation` | action | work_sdlc |
| `ready_for_implementation_review` | queue | work_sdlc |
| `implementation_review` | action (gate) | work_sdlc |
| `ready_for_shipment` | queue | work_sdlc |
| `shipment` | action | work_sdlc |
| `ready_for_shipment_review` | queue | work_sdlc |
| `shipment_review` | action (gate) | work_sdlc |
| `shipped` | terminal | work_sdlc |

### Gate SDLC
- `ready_to_evaluate` (queue), `evaluating` (action)

### Explore SDLC
- `ready_for_exploration` (queue), `exploration` (action)

### Lease SDLC
- `lease_ready` (queue), `lease_active` (action), `lease_terminated` (terminal)

### Cross-cutting
- `deferred` — passive escape (not terminal)
- `abandoned` — terminal

### ⚠ not a state
- `blocked` — only an edge kind (`blocked_by` / `blocks`), not a `KnotState`. The architecture doc's mention of a "blocked state" is legacy wording.

---

## Verbs

### add / remove edge <!-- auto -->
Create or delete a directed edge between two knots.
- `src/app/edges.rs:22` — `add_edge`
- `src/app/edges.rs:31` — `remove_edge`
- Subjects: pairs of knot ids plus a kind string.

### apply <!-- auto -->
Apply an incoming unit of change to local state. ⚠ **ambiguous** — subjects differ significantly:
- Events → cache: `src/app/rehydrate/apply_event.rs:12` — `apply_rehydrate_event`
- Git worktree → main branch: `src/sync/apply.rs:57` — `apply_to_head`
- State transition: `src/app/state_ops.rs:113` — `apply_state_transition_locked`
- Step history: `src/app/helpers.rs:380` — `apply_step_transition`
- Doctor fixes: `src/doctor_fix.rs:32` — `apply_fixes`
- Listing filters: `src/listing.rs:15` — `apply_filters`
- Snapshot restore: `src/snapshots.rs:141` — `apply_latest_snapshots`

### archive / cold sweep <!-- auto -->
Transition terminal knots into the cold catalog and out of the hot cache.
- `src/app/archival.rs:69` — `run_cold_sweep`
- Related: *rehydrate* (the reverse path).

### cascade <!-- auto -->
Propagate a parent knot's terminal transition down to its `parent_of` descendants.
- `src/app/state_ops.rs:198` — `cascade_terminal_state_locked`
- Related: `TransitionPlan::CascadeTerminal`.

### claim <!-- auto -->
Atomically grab a claimable knot, move it from its queue state into its action state, create/bind a lease, and return the action prompt.
- `src/poll_claim.rs:147` — `claim_knot`
- Related: *poll*, *peek*, *next*, *bind*.

### compact <!-- auto -->
Write append-only snapshots to shrink future cold-sync cost.
- `src/app/sync_ops.rs:153` — `compact_write_snapshots`
- `src/run_commands.rs:304` — `run_compact`

### create <!-- auto -->
Materialize a new knot. Variants:
- `create_knot()` — minimal (default workflow)
- `create_knot_in_workflow()` — specific workflow
- `create_knot_with_options()` — full (type, state, tags, plan, etc.)
- `src/app/knot_create.rs:23`, `:33`, `:51`

### doctor / fsck <!-- auto -->
Run repository health probes (`doctor`) or strict on-disk validation (`fsck`).
- `src/doctor.rs:97` — `run_doctor`
- `src/fsck.rs:55` — `run_fsck`

### evaluate <!-- auto -->
Decide a gate (`Yes` / `No`), optionally reopening linked knots on failure.
- `src/app/gate.rs:14` — `evaluate_gate`

### execute (operation) <!-- auto -->
Run a `WriteOperation` dispatched from the write queue. Subsidiary handlers cover plan edits, gate evaluation, rollback, step annotation, etc.
- `src/write_dispatch/execute/mod.rs:19` — `execute_operation`

### extend / terminate (lease) <!-- auto -->
Refresh a lease's expiry, or end it. `heartbeat` is the colloquial synonym for *extend* (driven by ordinary updates on the bound knot).
- `src/write_queue/lease_ops.rs:22` — `LeaseExtendOperation`
- `src/write_queue/lease_ops.rs:17` — `LeaseTerminateOperation`
- `docs/leases.md:59`

### init / uninit <!-- auto -->
Create (or tear down) the local store and, for git-mode, the remote `knots` branch.
- `src/init.rs:20` — `init_all`
- `src/init.rs:66` — `init_local_store`
- `src/app/sync_ops.rs:132` — `init_remote`

### install / uninstall <!-- auto -->
Register or remove a managed artifact. Subjects: workflow bundles, git hooks, shell completions, managed skills.
- `src/installed_workflows/operations.rs:51` — `install_bundle`
- `src/git_hooks.rs:180` / `:190` — `install_hooks` / `uninstall_hooks`
- `src/completions.rs:55` — `install_completions`

### next <!-- auto -->
Advance a knot from its current action state into the happy-path next queue state; ends any bound lease.
- `src/cli.rs:126`
- `README.md:184`

### peek <!-- auto -->
Inspect a knot without claiming it. Used by `kno poll` (no `--claim`) and `kno claim --peek`.
- `src/poll_claim.rs:89` — `peek_knot`

### plan edit (wave / step) <!-- auto -->
Mutate an execution plan: add / move / remove waves and steps.
- `src/app/execution_plan_edit.rs:26` — `plan_edit_wave_add` (and siblings)
- `src/domain/execution_plan_edit.rs:54` — `add_wave`
- `src/write_dispatch/execute/execute_plan_ops.rs:19` — top-level handlers

### poll <!-- auto -->
Peek at (or, with `--claim`, grab) the top claimable knot, respecting profile ownership and lease state.
- `src/poll_claim.rs:33` — `run_poll`

### pull / push / sync <!-- auto -->
- **pull**: fetch remote `knots`, reset worktree, apply index + full events to the cache, filtering out events for any knot this machine locally leases (see *quarantine*).
- **push**: copy local event files into the worktree, commit, push.
- **sync**: push then pull; both halves always run. Pull never defers wholesale on an active lease — it holds back only the knots that lease covers and reports them (`SyncSummary::held_back_knots`).
- `src/app/sync_ops.rs:18`, `:54`, `:80`
- ⚠ overloaded: `push_unique` in `src/installed_workflows/mod.rs:396` is unrelated — it appends an item to an internal list.

### reconcile <!-- auto -->
Re-derive a parent knot's state from its children after a child resolves.
- `src/app/state_ops.rs:93` — `reconcile_terminal_parent_state`
- `src/app/state_resolve.rs:11` — `reconcile_terminal_parent_state_locked`

### rehydrate <!-- auto -->
Rebuild a knot's hot-cache row by replaying its events from disk. Canonical term across docs and code — do not use `hydrate` as an alternative.
- `src/app/rehydrate.rs:47` — `rehydrate_from_events`
- `src/app/query.rs:224` — `rehydrate`

### resolve <!-- auto -->
Turn an input plus context into a canonical target. ⚠ **ambiguous** — subjects differ:
- Project context: `src/project.rs:257` — `resolve_context`
- Profile id: `src/app/profile_config.rs:75` — `resolve_profile_id`
- Next state: `src/dispatch.rs:31` — `resolve_next_state`
- Rollback target: `src/rollback.rs:16` — `resolve_rollback_state`
- Hooks dir: `src/git_hooks.rs:27` — `resolve_hooks_dir`
- Step metadata: `src/app/helpers.rs:64` — `resolve_step_metadata`

### rollback <!-- auto -->
Rewind a knot from its current action state back to the preceding ready state. The CLI verb is exposed; internally it uses `set_state` with a historical target resolved by `resolve_rollback_state`.
- `src/rollback.rs:16`
- `README.md:386`

### set_state <!-- auto -->
Transition a knot to a target state, enforcing profile transitions and (optionally) cascading terminals. Variants accept actor metadata and options.
- `src/app/state_ops.rs:24`, `:40`, `:60`

### update <!-- auto -->
Patch knot fields (title, body, description, acceptance, tags, priority, notes, invariants, gate data). Each patched field becomes one or more events.
- `src/app/knot_update.rs:29` — `update_knot_with_options`
- `src/app/types.rs:114` — `UpdateKnotPatch`

---

## Phrases

### "cold path" <!-- auto -->
The slower query/sync path that requires cold-catalog lookups or full-event rehydration. Opposite of *warm path*.
- `knots-architecture-v4.md`

### "cold tier" <!-- auto -->
The cold catalog plus its disk layout. `kno cold` is the CLI surface.
- `src/cli.rs:116`
- `knots-architecture-v4.md:285`

### "dead letter" <!-- auto -->
A knot stuck in a terminal-but-unresolved condition. Informal usage.
- `knots-architecture-v4.md`

### "fast-forward" <!-- auto -->
Remote-winning git update used when a push is non-fast-forward: local branch is reset to `origin/knots` after events are re-merged locally.
- `knots-architecture-v4.md:537`

### "fire to be tied" <!-- auto -->
The onboarding success line printed by `kno init`. Project in-joke, but shipped in user-facing output.
- `README.md:110`

### "handoff capsule" <!-- auto -->
Structured metadata attached as a knot note to pass context across workflow steps.
- `README.md:402`

### "hot window" <!-- auto -->
See the noun. Used as a phrase in prose when contrasting with cold tier.
- `knots-architecture-v4.md:287`

### "knot head" <!-- auto -->
See the noun. Index-event content.

### "lazy materialization" <!-- auto -->
Defer expiring/terminating a lease until the next access touches it, instead of scanning eagerly.
- `docs/leases.md:68`

### "passive escape state" <!-- auto -->
See the noun. README phrase for non-terminal waiting states.
- `README.md:36`

### "pull drift" <!-- auto -->
Condition where local unpushed events exceed `pull_drift_warn_threshold` — the doctor raises a warning.
- `src/app/types.rs:100`

### "push budget" <!-- auto -->
Configurable time budget for a push attempt (`push_retry_budget_ms`). If exceeded, the commit stays local.
- `src/db/migrations.rs:412`
- `knots-architecture-v4.md:597`

### "review hint" <!-- auto -->
Per-state instruction in step metadata telling a reviewer what to look at.
- `README.md:260`
- `src/profile.rs:31`

### "stale work" <!-- auto -->
In-flight work lost when an expired lease is reclaimed by another agent before the original finishes.
- `docs/leases.md:168`

### "sync pending" <!-- auto -->
Flag surfaced by doctor/sync when local changes have not yet been pushed.
- `knots-architecture-v4.md:348`

### "warm path" <!-- auto -->
The fast query path that reads only warm (id+title+state) data without rehydrating full event history.
- `knots-architecture-v4.md:269`

### "workflow-relevant index update" <!-- auto -->
An index-event change that advances the workflow etag (state, title, edges). Other changes don't bump the etag.
- `knots-architecture-v4.md:194`

---

## Acronyms & Shorthand

| Short | Expansion | Notes |
|-------|-----------|-------|
| CLI | Command-Line Interface | The `kno` / `knots` binary. |
| ETag | Entity Tag | Opaque version token used as a write precondition. |
| FIFO | First In, First Out | Queue discipline for the write queue. |
| If-Match | HTTP-style precondition | Encoded as `WorkflowPrecondition` on events. |
| OCC | Optimistic Concurrency Control | Realized via workflow etag + If-Match. |
| SDLC | Software Development Life Cycle | Suffix on every built-in workflow bundle. |
| ULID | Universally Unique Lexicographically Sortable Identifier | Accepted alternative to UUIDv7. |
| UUIDv7 | Time-ordered UUID | Default knot id format — gives sortable ids. |
| WAL | Write-Ahead Log | SQLite journal mode used by the cache. |

---

## Review Queue

Terms needing human attention. Resolve and remove once decided.

### Tracked as knots (do not duplicate here)

- ⚠ `apply` overload — tracked as knot `1b08` (Rename all uses of 'apply' to subject-specific verbs).
- ⚠ `KnotState` not generated from loom — tracked as knot `db35` (Generate KnotState from loom bundles).
- ⚠ Legacy state names (`idea`, `work_item`, `implementing`, …) in `knots-architecture-v4.md` — tracked as knot `1b0d` (Write knots-architecture-v5.md).

### Deferred (low priority / explicitly accepted)

- `resolve` — six distinct subjects (context, profile id, state, rollback target, hooks dir, step metadata). Accepted; each reads clearly in context.
- `push_unique` in `installed_workflows` — unrelated to git push but accepted; rename not prioritized.
- `Agent` across `GateOwnerKind::Agent`, `LeaseType::Agent`, and the generic actor concept — currently consistent; revisit only if drift appears.
- `blocked` — only an edge kind (`blocked_by` / `blocks`), not a state. The architecture-v5 knot will clarify prose wording.
