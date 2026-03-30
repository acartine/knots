# Knots Style Guide

## Limits
- Maximum file length: 500 lines.
- Maximum line length: 100 characters.
- Minimum test coverage: 95%.

## Notes
- Prefer small focused modules.
- Add tests for all new behaviors.

## Tracking Workflow
- Use Knots for issue tracking. Do not use Beads (`bd` commands).
- Preferred CLI command is `kno` (`knots` is a compatibility alias).
- Common commands:
  - `kno ls`
  - `kno show <knot-id>`
  - `kno new "<title>" -d "<description>"`
  - `kno update <knot-id> --status <state>`
  - `kno sync`

## Git Workflow
- Unless you are working on a specific knot step (where a worktree branch is expected), always
  commit your changes, push them, and make sure they are merged to main.
- Do not leave uncommitted or unpushed work at the end of a task.

## Source Code Size Standard

All source files under tracked directories must satisfy:

| Metric | Limit |
|--------|-------|
| File length | < 500 lines |
| Function/method body | < 100 lines |
| Line width | < 100 columns |

Enforcement: run `make lint` before merge. It must pass the configured
linter and size-checking script(s).

## Pre-Push Sanity (Required)
- Install the managed pre-push hook with `make install-hooks`.
- Do not push unless `make sanity` passes.
- `make sanity` runs formatting, lint, tests, and coverage checks.

## Coverage Ratchet Rule
- Coverage gate source of truth is `.ci/coverage-threshold.txt`.
- Never lower this threshold in a PR.
- Raise the threshold as coverage work lands until it reaches `95`.
