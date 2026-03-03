# kno

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
