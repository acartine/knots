# kno

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
