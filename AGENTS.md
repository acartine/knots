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
  - `kno sync`
