# kno

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
