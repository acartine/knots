# Knots Style Guide

## Limits
- Maximum file length: 500 lines.
- Maximum line length: 100 characters.
- Minimum test coverage: 90%.

## Notes
- Prefer small focused modules.
- Add tests for all new behaviors.

## Tracking Workflow
- Use Knots for issue tracking. Do not use Beads (`bd` commands).
- Preferred CLI command is `kno` (`knots` is a compatibility alias).
- Common commands:
  - `kno ls`
  - `kno new "<title>" --state work_item`
  - `kno update <knot-id> --status implementing`
  - `kno show <knot-id>`
  - `kno wf list`
  - `kno sync`

## Pre-Push Sanity (Required)
- Install the managed pre-push hook with `make install-hooks`.
- Do not push unless `make sanity` passes.
- `make sanity` runs formatting, lint, tests, and coverage checks.

## Coverage Ratchet Rule
- Coverage gate source of truth is `.ci/coverage-threshold.txt`.
- Never lower this threshold in a PR.
- Raise the threshold as coverage work lands until it reaches `95`.
