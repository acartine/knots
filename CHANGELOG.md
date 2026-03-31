# kno

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
