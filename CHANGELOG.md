# kno

## 0.16.4

### Patch Changes

- 6f4d135: Repair Knots installer and upgrade rails so latest installs resolve through the
  GitHub Releases API before falling back to the web redirect, and `kno upgrade`
  defaults to the running binary's install directory.

## 0.16.3

### Patch Changes

- a2289e9: Add Diffinite-safe Knots sync refs, including `refs/work/knots` defaults,
  `kno sync-ref migrate`, and doctor diagnostics for stale legacy
  `refs/heads/knots` state.

## 0.16.2

### Patch Changes

- 749f0ee: **Bug fixes**

  - Fix workflow id parity doctor repairs to correctly handle workflow ID mismatches
  - Fix stale artifact reaper freshness check to properly detect and clean stale local build artifacts
  - Fix managed_skills test env_lock race between tests and tests_ext modules

  **Improvements**

  - Notice managed skill update changes — surface when managed skills are updated
  - Reap stale local build artifacts on clean builds
  - Document lease-backed skill handoffs in agent skills

## 0.16.1

### Patch Changes

- fade01e: Fix synced terminal knots so they stay in the hot tier during the 72-hour grace
  window, keeping recently shipped or abandoned work list-visible. `kno doctor --fix`
  now also repairs recent terminal rows that were incorrectly placed in the cold
  catalog.

## 0.16.0

### Minor Changes

- e88c718: Add first-class verification steps to knots. Use `kno new --verification-step "<step>"` (repeatable) when creating a knot, and manage them on existing knots with `kno update --add-verification-step`, `--remove-verification-step`, and `--clear-verification-steps`. The steps are surfaced in `kno show` and in `kno show --json` under `verification_steps`.

### Patch Changes

- e88c718: Fix execution plan cache replay loss so plan-step edits (including removals) are preserved correctly when the local cache rehydrates from the event log.

## 0.15.15

### Patch Changes

- 7bf6ad2: Skip imported knot heads referencing unknown workflows during sync instead of failing. A warning is emitted with the knot id and workflow id so the workflow can be installed in the repository, and follow-on full events for skipped knots are ignored.

## 0.15.14

### Patch Changes

- 5aa9a38: Add scope metadata support to knot creation and views, including CLI flags for setting project,
  area, tags, owner, priority, due dates, and external references.

  Keep active knots in the hot cache during sync so recently updated work stays visible in warm
  queries.

  Repair managed-skills doctor fixes so ignored skill paths are handled correctly.

## 0.15.13

### Patch Changes

- 0c9693e: Add a `nested_caches` doctor check that warns when cached `.knots` directories
  are nested inside the canonical store. The warning lists each detected cache and
  prints the manual `rm -rf <path>` cleanup command so users can resolve silent
  state-drift risks deliberately.
- aa52fde: Rename `kno skill <knot-id>` to `kno prompt <knot-id>` to remove the naming
  collision with `kno skills` (the managed-skill installer). The old form still
  works as a hidden alias for backward compatibility, but emits
  `warning: 'kno skill' is deprecated; use 'kno prompt'` on stderr.

## 0.15.12

### Patch Changes

- 9fbf49e: Add `--e2e` support to `kno claim` and `kno poll` so end-to-end agent runs can
  receive an explicit `e2e_continuation` workflow boundary. JSON claim output now
  includes `e2e` and `workflow_boundary_kind`, while ordinary claims continue to
  emit the default `single_action` boundary.
- 597fbcf: Clarify the `planning` workflow prompt so agents create implementation child
  knots without blocking the parent's `plan_review` transition. The prompt now
  explicitly recommends `kno q "<title>"`, `kno new "<title>" -f`, or
  `kno new "<title>" -p autopilot_no_planning` for child knots created during
  planning, documents the create-then-link order with
  `kno edge add <parent> parent_of <child>`, and adds a Hierarchy Gate note
  explaining why a child created with the default `autopilot` profile (rank 1)
  blocks the parent's `plan_review` transition (rank 3). Hierarchy gate
  semantics are unchanged.
- 9fbf49e: Enrich `kno ls --json` output for active action knots with bound lease agent
  metadata. The `lease_agent` object now mirrors `kno show --json` by including
  provider, agent, model, and version details when a lease is bound, while queued
  knots without a bound lease continue to omit `lease_agent`.
- 9fbf49e: Preserve tag casing in knot creation, updates, event replay, and sync while
  continuing to treat duplicate tags case-insensitively. Tags such as `Alpha`
  now display with the casing users entered instead of being normalized to
  lowercase.

## 0.15.11

### Patch Changes

- bd5a3eb: Publish GitHub Release notes from the Changesets-built `CHANGELOG.md` entry for
  the shipped version. The release workflow now uses that changelog section as
  the release body, so releases show the actual user-facing changes instead of
  only the `Version Packages` PR merge.
- 2b46132: Fix `kno sync push` for projects that use a configured Knots store root, so
  local event files are found and published from the active store instead of only
  from `.knots/` under the repository root.

  Clarify sync push progress output so a no-op push reports that local files are
  being checked, and only reports copied files when files actually need to be
  copied into the publish worktree.

## 0.15.10

### Patch Changes

- dc11466: Fix managed skills gitignore rules so `.agents` and `.claude` are recursively
  ignored except for their `skills` directories. `kno doctor --fix` now also
  repairs legacy managed-skills gitignore rules when skills are already installed.

## 0.15.9

### Patch Changes

- 7a4e510: Fix gitignore setup for Knots-managed project directories. `kno init` now
  ensures `.knots` is ignored, while `kno skills install` ignores `.agents` and
  `.claude` contents except for their allowlisted `skills` directories.

## 0.15.8

### Patch Changes

- 8af7518: Tag, query, profile, and `--all` filters now apply before pagination on
  `kno ls --json`. Previously, only `--state` and `--type` were pushed into
  SQL, while tag/query filters ran after `LIMIT`/`OFFSET` was applied. As a
  result, `kno ls --tag <tag> --json --limit 1` could return an empty data
  array even when matches existed, and `total` reported the unfiltered hot
  count instead of the filtered match count. The paginated path now
  materializes the full hot tier, applies the same filter pipeline as the
  non-paginated path, sets `total` to the filtered count, and slices the
  filtered list by `offset`/`limit`. Pages are stable across offsets and
  `has_more` is consistent with `total` and `data.length`.
- becf694: `kno ls --json` now reports consistent pagination metadata for text queries
  that filter out every row on the requested page. Previously, the SQL layer
  paginated before the in-memory query filter ran, so a query with no matches
  could return `data: []` alongside `total: 50` and `has_more: true`. The CLI
  now applies all filters before paginating, so empty matches report
  `total: 0` and `has_more: false`.

## 0.15.7

### Patch Changes

- d59ca7d: Knots commands invoked from a Git linked worktree now share the canonical
  repo `.knots` store with the primary worktree. Previously the post-merge
  sync hook tried to check out the `knots` branch into a per-worktree
  `.knots/_worktree` and failed with `fatal: 'knots' is already used by
worktree at '<primary>/.knots/_worktree'`. The store path is now resolved
  via `git rev-parse --git-common-dir` so all linked worktrees of the same
  repo see one Knots store.

## 0.15.6

### Patch Changes

- ce81baa: `kno doctor` no longer warns forever about `cold_tier_imbalance`. The check
  now measures three real tier invariants — disjointness (no id in both hot and
  cold), cold-is-terminal-only, and no stale-terminal hot rows — instead of
  treating "fewer than 100 hot knots and a non-empty cold catalog" as a problem.
  A repo whose cold tier holds only legitimately-old shipped/abandoned knots is
  healthy. `kno doctor --fix` restores each invariant idempotently and the next
  sync, sweep, and snapshot bootstrap all uphold them, so doctor stays `pass`.
  See `docs/tier-balance.md`.
- de08eac: Fix `kno sync` dropping descriptions for knots created with `kno new -d`. Create
  now emits a separate `knot.description_set` event so the standard apply path
  populates description on the receiving host. The sync-apply and rehydrate paths
  also gained a backward-compat read of the inline `body` field on `knot.created`
  events so descriptions on knots created before this fix are recovered on the
  next pull. The compat reads will be removed once the pre-fix event cohort ages
  out (tracked by knot `83b1`).

## 0.15.5

### Patch Changes

- 564671a: Execution plan knots now require an objective at creation and on update. `kno
new` and `kno update` reject execution-plan knots that lack an objective, and
  the execution-plan docs describe the field. Existing non-execution-plan knots
  are unaffected.
- 564671a: Gate evaluation prompts now carry the knot's acceptance criteria, so the
  evaluator sees the same contract the author wrote. Prompt resolution threads
  acceptance through `gate_sdlc/prompts/evaluating.md` and the knots prompt
  pipeline; no flag changes, but evaluations gain context automatically on the
  next claim.
- 564671a: Agent identity is now stamped from the bound lease across every write sink
  (next, rollback, claim, poll-claim, gate evaluate, step annotate, state, note,
  handoff). The per-command `--agent-name`, `--agent-model`, `--agent-version`,
  `--note-agent-*`, and `--handoff-agent-*` flags are still accepted for
  compatibility but are runtime-ignored and emit a deprecation warning on stderr;
  their help text is prefixed `[DEPRECATED — IGNORED]`. The deprecation warning
  uses singular or plural wording depending on how many flags were supplied. `kno
lease create` is unchanged — the lease remains the single declared source of
  agent identity.

## 0.15.4

### Patch Changes

- f035fc3: `kno doctor --fix` now emits per-operation progress lines so long-running
  repairs no longer look like a silent hang. The command announces the
  diagnostic phase, prints a `Fixing <check>...` line as each fix starts, and
  prints a `N issue(s) fixed.` summary on completion. Running `kno doctor`
  without `--fix` and `kno doctor --fix --json` are unchanged.

## 0.15.3

### Patch Changes

- 9861dcd: Fix `kno doctor --fix` leaving `workflow_id_parity` (and other event-log
  repairs) perpetually warning after a fix run. Repair events are emitted
  into the local `.knots/index/` store, but the check scans the shared
  `_worktree`, so the post-fix recheck never saw them. `apply_fixes` now
  reports whether the event log was touched, and `run_doctor_with_fix_at`
  publishes the repair events via a best-effort sync before the recheck.
  Also prevents each subsequent `kno doctor --fix` from stacking duplicate
  repair events for the same stale knots while waiting for a sync.
- 33cbe7d: Fix `kno ls --type <type>` silently missing knots first materialized by
  a `kno sync` pull. The sync-apply path never read `type` from
  `idx.knot_head` event data when upserting a new cache row, so every
  knot authored on another machine landed in the local `knot_hot` table
  with `knot_type` NULL or empty — `kno ls --type execution_plan` (for
  example) filtered them out.

  Two changes:

  - `build_index_upsert` now reads `data.type` from the index event and
    falls back to the existing cached value only when the event omits
    it. Future pulls populate `knot_type` correctly on first apply.
  - New `knot_type_backfill` doctor check + `--fix` path backfills
    already-corrupted caches by scanning the worktree's latest
    `idx.knot_head` event per affected knot and writing the type back to
    `knot_hot`. No event emission — purely a local cache repair.

- ff4e566: Fix `kno rehydrate` and `kno doctor --fix`'s cold-tier sweep silently
  failing for knots pulled from origin. `rehydrate_from_events` only
  scanned the local `.knots/events/` and `.knots/index/` directories, but
  events arriving via `kno sync` pull land in `.knots/_worktree/.knots/`
  and are never copied into the local store. Rehydrating such a knot
  failed with `missing workflow_id`, and `fix_cold_tier_imbalance`
  swallowed the per-knot error — so the doctor warning persisted with
  zero rehydrations even when there were plenty of cold candidates.

  `rehydrate_from_events` now accepts multiple store roots and callers
  pass both the local and worktree roots, deduped by relative filename
  so locally-mirrored events aren't replayed twice.

- b61cd69: Fix `kno sync` hard-failing on legacy events that predate the
  2026-04-09 strictness change ("Remove legacy workflow runtime
  fallbacks"). Two tolerant fallbacks are restored at apply time so a
  bootstrap pull of a pre-cutoff repo no longer errors out with
  `missing 'profile_id' string field`:

  - `required_profile_id` defaults to `"autopilot"` when the field is
    missing or empty, instead of erroring. Silent; the event log stays
    intact and the cache row carries the modern default.
  - `required_workflow_id` now recognizes the pre-registry name
    `"default"` as a legacy value alongside `"compatibility"` and
    `"knots_sdlc"`, translating it to `"work_sdlc"` at apply time with
    the same one-time warning pattern.

  Event files on disk are not rewritten — this is read-time translation,
  consistent with the existing legacy-value handling.

## 0.15.2

### Patch Changes

- fb9e855: Deprecate `--agent-name`, `--agent-model`, and `--agent-version` on `kno claim`.
  The flags still work and continue to stamp metadata on the auto-created lease,
  but the canonical pattern is now `kno lease create` followed by `kno claim
--lease <id>`. Using the deprecated flags emits a warning to stderr and they
  will be removed in a future release.
- 65c23c0: Add a `workflow_id_parity` doctor check and `--fix` path. The check scans the
  pulled worktree for knots whose latest `idx.knot_head` event lacks
  `workflow_id`. `kno doctor --fix` publishes a minimal repair event per stale
  knot so the shared event log eventually reaches parity with modern events —
  active knots use the local DB state, archived knots use the cold catalog plus
  the workflow inferred from the stale event's knot type.
- 65c23c0: Fix `kno sync` failing on legacy `idx.knot_head` events written before
  `workflow_id` became a required field. When `workflow_id` is missing, sync now
  infers it from the event's knot type (defaulting to `work_sdlc` for `work`
  knots), matching the existing `parse_knot_type` fallback convention. A
  one-shot warning reports the inference.

## 0.15.1

### Patch Changes

- 8bd262d: Fix builtin workflow profile lookup so execution plan knots advance through the
  correct workflow family.

## 0.15.0

### Minor Changes

- 17f7777: ### Features

  - add native execution plan knots, persistence, and CLI editing flows
  - add orchestration workflow support for execution plans
  - add `kno new --tag` for tagging knots at creation time
  - add `cold_tier_imbalance` doctor checks and repair paths

  ### Fixes

  - migrate workflow state handling away from the legacy `KnotState` enum
  - accept legacy execution plan `beat_ids` during compatibility transitions
  - tighten cold-tier archival behavior and coverage around execution plan flows

## 0.14.0

### Minor Changes

- 82f7000: ### Features

  - Add NDJSON streaming output for `kno ls --stream`
  - Add SQL-level pagination with `kno ls --limit` and `--offset` flags
  - Add exploration workflow for lightweight investigations
  - Add refine-knot-scope managed skill
  - Support Codex project-level skills in `.agents/skills/`
  - Add managed knots-create skill
  - Add explore knot type with renamed builtin workflows
  - Auto-register builtin workflows when config is missing
  - Emit "no claimable knots found" in `poll --json` mode

  ### Fixes

  - Handle workflow ID mismatches during sync gracefully
  - Skip step metadata enrichment for unknown profiles
  - Fix loom owner projection
  - Repair legacy builtin workflow IDs
  - Fix integration tests for project-only Codex skills

  ### Chores

  - Remove legacy workflow runtime fallbacks and compatibility aliases

## 0.13.1

### Patch Changes

- f33f2d1: - Add lease timeout with lazy expiry and heartbeat
  - Restrict lease binding to claim flow
  - Hide lease IDs from generic show output
  - Improve read tracing, sync dedup, and cache-miss behavior
  - Tighten lease enforcement for claims and next
  - Materialize expired leases, preserve heartbeat timeout, harden next exception
  - Remove auto-sync, fix worktree discovery, and scope lease materialization
  - Cover unknown lease state fallback to stabilize coverage

## 0.13.0

### Minor Changes

- be6dbde: - Add ArtifactTarget enum for per-step output validation
  - Add branch and live_deployment output targets to prompt templates
  - Embed builtin compatibility workflow as a Loom bundle
  - Remove static prompt fallback from claim poll and skill (Loom-first prompts)
  - Add owner and artifact metadata to knot views and workflows
  - Render built-in prompts per profile with delivery targets
  - Add worktree recovery hint to knot not-found errors
  - Centralize workflow prompt lookup
  - Extensive e2e test coverage for Loom prompt resolution and output targets

## 0.12.0

### Minor Changes

- 0f69a0e: - Add periodic CLI upgrade notice: the CLI now checks for newer releases and
  prompts the user to upgrade when one is available
  - Add multi-workflow defaults and note selection
  - Fix: suppress upgrade hook hint when hooks are already current

## 0.11.0

### Minor Changes

- 2a35dbf: - Add periodic CLI upgrade notice that prompts users when a newer version is available
  - Add multi-workflow defaults and note selection for improved workflow UX
  - Fix: suppress upgrade hook hint when hooks are already current

## 0.10.2

### Patch Changes

- 3ca5f6b: Add named projects and confirmed project deletion

## 0.10.1

### Patch Changes

- 7fbe752: - Restrict Claude skills to project root directory

## 0.10.0

### Minor Changes

- 77fc354: - Add ship-release skill for autonomous release cutting
  - Update loom SDLC workflow definitions (simplified workflow, updated prompts)
  - Remove changeset enforcement from pre-push hook and CI

## 0.9.1

### Patch Changes

- 72f62ae: Replace post-upgrade hint to suggest `kno doctor` instead of `kno hooks install`.
- d5bab4b: Update workflows to be more precise in profile and implementation handling.

## 0.9.0

### Minor Changes

- dd44d2b: Add first-class knot acceptance criteria with native storage, sync, and CLI support.
- f54ca96: Defer `kno sync` when active leases exist instead of erroring. The sync is queued via sync_pending and automatically triggered when the last active lease is terminated.
- f445d27: Add lease threading to claim and next commands. External lease IDs can now be passed via --lease flags so the calling process can thread its own lease through the workflow instead of having duplicate leases created.

### Patch Changes

- 6c090d3: Detect managed skill drift in `kno doctor` by comparing deployed managed
  `SKILL.md` files against rendered content. `kno doctor --fix` now reconciles
  both missing and drifted managed skills.
- fe62290: Harden self-update curl failures and remove auto-resolved handoff metadata flags from skill files.
- f1a082a: Fix `kno skills update` to only write to the preferred (project-level) location instead of all installed locations.
- 0e9d348: Add missing test assertions for worktree guidance in managed skill prompts.

## 0.8.0

### Minor Changes

- 2129bcf: Enforce hierarchy state progression: parent knots cannot advance
  past the state of any child knot. Attempts return an error with
  the list of blocking descendants.
- 514e786: Add `kno loom compat-test` with a Loom workflow compatibility harness, bundle failure-path
  runtime support, and JSON bundle metadata preservation for named prompt outcomes.
- f53ea7c: Add Lease knot type for agent session tracking with `--lease` flag, note
  auto-stamp, and `--json` output. Add Gate knot type with evaluation flow,
  failure reopen, and generalized rollback evaluation states. Add `kno rollback`
  and `kno rb` commands for action states. Add colored sync progress feedback.
  Display relationships grouped by kind in `kno show`. Auto-resolve terminal
  parent knots with doctor fix. Install Loom workflow bundles. Add worktree
  guidance to managed skill prompts.

### Patch Changes

- efa1290: Stabilize CLI integration tests under tarpaulin by using robust binary lookup.
- 423747b: Fix terminal parent auto-resolution when a child is deferred, and allow
  deferred knots to move directly into terminal states during approved
  hierarchy cascades.
- b6b3174: Add a CLI regression test covering managed-skill doctor checks and `kno doctor --fix`.
- 10d7281: Fix upgraded installs that still have legacy `{}` lease cache payloads so
  commands like `show --json` continue to work after the lease metadata schema
  expands.
- 71e5e12: Make `kno loom compat-test` self-contained by embedding the knots_sdlc loom template and
  removing the source path argument. Fix JSON bundle param deserialization for loom 0.2.0.
- 5e02e81: Improve `kno loom compat-test` text feedback and directory-aware diagnostics.
- 90762f4: Fix managed skill installation so `kno skills` deploys the `knots` and
  `knots-e2e` skills with exact path reporting and doctor guidance.
- 009f6cf: Improve parent-knot prompts and managed skill guidance so agents handle child
  knots before advancing or rolling back the parent.
- a3eb406: Pin the Rust toolchain used by CI and local sanity checks so pre-push formatting runs are deterministic.
- fe4b069: Resolve the default cache database path relative to `--repo-root` so
  `kno -C <repo> show <id>` works from outside the target repository.
- 069ce46: Allow parent to move to a terminal state without cascade approval when
  all descendants are already in the target state.

## 0.7.6

### Patch Changes

- 3ae6571: Tighten agent skill prompts so each workflow step has a clear
  single-step boundary, explicit stop condition, and fewer
  cross-stage instructions.

## 0.7.5

### Patch Changes

- Improve clone bootstrap and release version syncing

  - Make `kno init` detect an existing `origin/knots` branch, pull knots from the
    remote into a fresh clone, and continue installing managed hooks
  - Refresh and verify `Cargo.lock` during release version sync so version bumps do
    not leave the lockfile dirty for the next Cargo command

## 0.7.4

### Patch Changes

- d94a6af: Improve show/claim output and fix edge cases

  - Display the latest note and handoff capsule in `show` and `claim` output,
    with a hint to use `-v`/`--verbose` to see all entries
  - Guard `claim`/`peek` on queue state instead of relying on happy-path traversal
  - Tighten metadata hints in `show` and `claim`
  - Detect user's actual shell when `$SHELL` is unset
  - Refine implementation review guidance

## 0.7.3

### Patch Changes

- 2fafb6b: - Detect stale lock files via PID and reduce lock timeout from 30s to 5s
  - Resolve latest version via redirect instead of GitHub API to avoid rate limits
  - Auto-add install directory to PATH in shell rc file

## 0.7.2

### Patch Changes

- 65fdc2b: - Make install.sh POSIX-compatible so `curl | sh` works on Debian/Ubuntu (dash)
  - Add linux-aarch64 release build and installer support

## 0.7.1

### Patch Changes

- 00cfc1a: - Implement step history model for tracking state transitions with timestamps
  - Resolve partial hierarchical aliases like `ba0e.2`
  - Add `kno create` as alias for `kno new`
  - Add dirty-workspace failure mode to shipment review skill

## 0.7.0

### Minor Changes

- 19aa455: ### Features

  - Add invariants field to knots with full lifecycle support: Invariant type
    (Scope/State), event sourcing, SQLite v7 migration, CLI flags
    (`--add-invariant`, `--remove-invariant`, `--clear-invariants`), UI display,
    and prompt rendering.

  ### Fixes

  - Relax self_manage test assertion for coverage tool compatibility.
  - Fix `--handoff-date` to `--handoff-datetime` in skill templates.

  ### Docs

  - Add handoff capsules with full agent metadata to all skill prompt paths
    (success and failure modes).

  ### Tests

  - Add invariant CLI flag, model serialization, persistence round-trip, and
    sync/apply coverage tests.
  - Improve coverage collection and integration test binary resolution.

## 0.6.3

### Patch Changes

- 1f8ae4f: - Warn on pull when local event drift exceeds threshold
  - Enforce read-only constraints in skill review steps
  - Require short commit hash tagging in skills
  - Update README to reflect current claim/next CLI flags

## 0.6.2

### Patch Changes

- b7eaa89: Fix doctor to detect and fix stale/orphaned hooks. check_hooks now warns on
  outdated hook content and leftover legacy hooks (e.g. post-commit). doctor --fix
  removes legacy hooks before reinstalling current managed hooks.

## 0.6.1

### Patch Changes

- 6558089: ### Fixes

  - Remove `post-commit` from managed hooks to prevent recursive fork bomb
    where each sync commit spawned another background `kno sync`.
  - Change hook template from backgrounded `kno sync` to foreground `kno pull`
    so errors are visible.
  - Add `--no-verify` to internal sync commits to prevent hook recursion while
    locks are held.

## 0.6.0

### Minor Changes

- a769b11: ### Features

  - Add `--expected-state` optimistic guard to `kno next`, making state
    progressions idempotent and preventing stale updates from clobbering
    concurrent changes.
  - Add git hooks (post-commit, post-merge, post-checkout) for automatic
    knot sync on git operations.
  - Add `doctor --fix` remediation flow that can automatically resolve
    detected issues such as version mismatches.
  - Add `commit:<hash>` tagging instructions to skill prompts and enforce
    commit tag validation in shipment review.

  ### Fixes

  - Fix `doctor --fix` version remediation to run correctly in-process.

  ### Chores

  - Polish doctor and upgrade output formatting.
  - Stabilize sync and hooks test coverage paths.
  - Additional test coverage for doctor fix, upgrade summary, and color
    fallback.

## 0.5.0

### Minor Changes

- a1eb0d4: Add structured JSON output and agent metadata to kno next

  - `kno next` now supports `--json` to emit structured JSON containing
    the knot id, previous state, new state, and owner_kind.
  - All `kno next` calls in skill prompts include agent metadata flags
    (`--actor-kind`, `--agent-name`, `--agent-model`, `--agent-version`).
  - Agent metadata is included in claim completion commands.
  - Eliminated unsafe env var manipulation from all tests in favor of
    injectable overrides.
  - Fixed sync test time drift by widening hot_window_days.

## 0.4.0

### Minor Changes

- 30cf4b7: Add version check to `kno doctor` that verifies the installed CLI version
  matches the latest published release.

## 0.3.0

### Minor Changes

- 6ea8d04: Add `--peek` flag to `kno claim` that shows the claim output without advancing knot state.

## 0.2.2

### Patch Changes

- 0ae389c: Switch poll and claim completion guidance to `kno next --actor-kind agent` and add
  `--actor-kind`, `--agent-name`, `--agent-model`, and `--agent-version` to `kno next`.

## 0.2.1

### Patch Changes

- f633514: Fix `kno sync` failing when a pre-push hook is installed by adding `--no-verify` to the internal knots branch push.
- b534ff2: Refinement of skills to eliminate hardcoding local project bias.

## 0.2.0

### Minor Changes

- f3273c7: Add M2.7 field parity and migration readiness with:

  - `kno update` patch command for title, description, priority, status, type, tags,
    notes, and handoff capsules.
  - first-class `notes[]` and `handoff_capsules[]` metadata arrays
    (`username/datetime/agentname/model/version`).
  - SQLite migration v3 parity fields and backfill from legacy body/notes.
  - import and sync reducers updated for parity mapping and metadata event handling.

- 1a10eba: Add public repo readiness, release automation, and curl installer
  infrastructure before M3.
