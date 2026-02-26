# Knots

[![CI][ci-badge]][ci-url]
[![Coverage][coverage-badge]][coverage-url]
[![License: MIT][license-badge]][license-url]

Knots is a local-first, git-backed Agentic Memory Management Framework designed for fast local workflows with append-only events and a SQLite cache.

# Install with curl
The installer pulls from GitHub Releases and installs to `${HOME}/.local/bin` by default.

Latest release:
```bash
curl -fsSL https://raw.githubusercontent.com/acartine/knots/main/install.sh | sh
```

# Why Knot
Yet another home-rolled agent memory thing when we already have Beads.  :-)

Its purpose is to provide a fast, opinionated workflow and responsibility enforcer.  Each knot allows for human-gating or agentic-delegation at each step of the workflow.  You decide what you need to see and what you don't.

# Basic Concepts
## Actions and Queues
Each step of the workflow is either an Action or a Queue.
- Action states are "In Progress".  They cannot be assigned.
- Queue states are, obviously, the opposite.

This makes it easy to see what needs to be done, and what is being worked on.

## Profiles
### Action Ownership and Output
Knots provides one workflow - but several profiles.  A profile assigns ownership to actions,
and in some cases it defines the output of action steps. This means you can have an Implementation Review step that is human gated, where the input is a branch, a PR, or a merged commit (gasp).  This provides granular control over what agents can do along with a a definition of done.

### Knot-Level Profiles
This means you can have different profiles for different knots.  You can decide if something is a 
small patch that skips planning and review, or a full-blown feature that goes through the full workflow.


# Other Commands
Verify install:
```bash
kno --version
```

Update installed binary:
```bash
kno upgrade
kno upgrade --version v0.2.0
```

Uninstall installed binary:
```bash
kno uninstall
kno uninstall --remove-previous
```

## Core usage
Create a knot:
```bash
kno new "Document release pipeline" --state ready_for_implementation
kno new "Triage regression"                  # uses repo default profile
kno new "Hotfix gate" --profile semiauto
```

Update state:
```bash
kno state <knot-id> implementation
```

Patch fields with one command:
```bash
kno update <knot-id> \
  --title "Refine import reducer" \
  --description "Carry full migration metadata" \
  --priority 1 \
  --status implementation \
  --type task \
  --add-tag migration \
  --add-note "handoff context" \
  --note-username acartine \
  --note-datetime 2026-02-23T10:00:00Z \
  --note-agentname codex \
  --note-model gpt-5 \
  --note-version 0.1
```

List and inspect:
```bash
kno ls
kno ls               # shipped knots hidden by default
kno ls --all         # include shipped knots
kno ls --state implementation --tag release
kno ls --profile semiauto
kno ls --type task --query importer
kno show <knot-id>
kno show <knot-id> --json
```

Sync from dedicated `knots` branch/worktree:
```bash
kno sync
```

Manage dependency edges:
```bash
kno edge add <src-id> blocked_by <dst-id>
kno edge list <src-id> --direction outgoing
kno edge remove <src-id> blocked_by <dst-id>
```

Import supports parity fields when present:
- `description`, `priority`, `issue_type`/`type`
- `labels`/`tags`
- `notes` as legacy string or structured array entries
- `handoff_capsules` structured array entries

# Developing
For information on the release process and local development testing, please see [CONTRIBUTING.md](CONTRIBUTING.md).


## Security and support
- Security policy: see `SECURITY.md`
- Non-security bugs/feature work: open a normal GitHub issue
- Installation/release regressions: open issue with logs and platform details

### Enable private vulnerability reporting (GitHub)
After publishing the repository:
1. Open repository `Settings`.
2. Open `Security & analysis`.
3. Enable `Private vulnerability reporting`.
4. Confirm `SECURITY.md` is discoverable from the repository root.

## License
MIT. See `LICENSE`.

[ci-badge]: https://github.com/acartine/knots/actions/workflows/ci.yml/badge.svg
[ci-url]: https://github.com/acartine/knots/actions/workflows/ci.yml
[coverage-badge]: https://codecov.io/gh/acartine/knots/graph/badge.svg?branch=main
[coverage-url]: https://codecov.io/gh/acartine/knots
[license-badge]: https://img.shields.io/badge/License-MIT-yellow.svg
[license-url]: https://opensource.org/licenses/MIT
