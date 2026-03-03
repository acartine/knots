---
"knots": minor
---

### Features

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
