# Knots

[![CI][ci-badge]][ci-url]
[![Coverage][coverage-badge]][coverage-url]

Knots is a local-first, git-backed issue tracker designed for fast local workflows with
append-only events and a SQLite cache.

## Why Knots
- Keep issue data out of normal code PR diffs.
- Work offline first and sync through git when needed.
- Keep `new`, `state`, `ls`, and `show` fast by reading local cache data.
- Import existing history from JSONL (source-only import).

## Quickstart
### Prerequisites
- Rust (stable)
- Git
- SQLite is bundled through `rusqlite` (`bundled` feature enabled)

### Build from source
```bash
cargo build --release
cargo run -- --help
```

### Run with a local repo + cache path
```bash
kno --repo-root . --db .knots/cache/state.sqlite ls
```

## Milestone status
- M0 Workspace/Tracking: complete
- M1 Local Event + Cache Core: complete
- M1.5 Import Ingestion: complete
- M2 Dedicated Branch Sync: complete
- M2.5 Public Repo Readiness: complete
- M2.6 Release + Curl Install: complete
- M2.7 Field Parity + Migration Readiness: complete
- M3+ Tiering, concurrency, and operability: pending

## Install with curl
The installer pulls from GitHub Releases and installs to `${HOME}/.local/bin` by default.

Latest release:
```bash
curl -fsSL https://raw.githubusercontent.com/acartine/knots/main/install.sh | sh
```

Pinned release:
```bash
curl -fsSL https://raw.githubusercontent.com/acartine/knots/main/install.sh \
  | KNOTS_VERSION=v0.1.0 sh
```

Custom install directory:
```bash
curl -fsSL https://raw.githubusercontent.com/acartine/knots/main/install.sh \
  | KNOTS_INSTALL_DIR="$HOME/.knots/bin" sh
```

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
Create an issue:
```bash
kno new "Document release pipeline" --state work_item
kno new "Triage regression"                  # uses repo default workflow
kno new "Hotfix gate" --workflow human_gate
```

Update state:
```bash
kno state <knot-id> implementing
```

Patch fields with one command:
```bash
kno update <knot-id> \
  --title "Refine import reducer" \
  --description "Carry full migration metadata" \
  --priority 1 \
  --status implementing \
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
kno ls --state implementing --tag release
kno ls --workflow automation_granular
kno ls --type task --query importer
kno show <knot-id>
kno show <knot-id> --json
```

Workflow inspection:
```bash
kno workflow list     # or: kno wf list
kno workflow show automation_granular
kno workflow show human_gate
kno workflow set-default human_gate
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

## Workflow definitions
Workflow definitions are embedded in the `kno` CLI and are not read from repo-local
`.knots/workflows.toml`.

Notes:
- Workflow ids and states are normalized to lowercase.
- Use transition `from = "*"` for wildcard transitions.
- Built-ins:
- `automation_granular`: full state graph optimized for automation.
- `human_gate`: coarse graph with a human checkpoint after PR creation.
- `kno init` prompts you to set the repo default workflow.

## Import from Beads
Export Beads to JSONL, then import.

```bash
bd sync --flush-only
kno import jsonl --file .beads/issues.jsonl
kno import status
```

Import supports parity fields when present:
- `description`, `priority`, `issue_type`/`type`
- `labels`/`tags`
- `notes` as legacy string or structured array entries
- `handoff_capsules` structured array entries

## Release process
Knots uses Changesets to manage release metadata.

1. Add a changeset for user-facing changes.
2. Merge to `main`.
3. Changesets workflow opens/updates a `Version Packages` PR.
4. Merge version PR.
5. Release workflow builds binaries, creates release assets, and publishes tag `v<version>`.

Published assets:
- `knots-v<semver>-darwin-arm64.tar.gz`
- `knots-v<semver>-linux-x86_64.tar.gz`
- `knots-v<semver>-checksums.txt`

### Local release/install smoke test
Run the installer smoke script before publishing major release process changes:

```bash
scripts/release/smoke-install.sh
```

The script validates both latest and pinned install flows and confirms `kno.previous` is
retained after reinstall. It also verifies the installed binary exactly matches the local
`cargo build --release` output (version + SHA-256 hash).

Optional smoke test env vars:
- `KNOTS_SMOKE_INSTALL_DIR=/absolute/path` keeps the installed binary at a persistent location.
- `KNOTS_SMOKE_KEEP_TMP=1` retains temporary tarball/server artifacts after the run.

### Toggle between release and local test binaries
Use channel scripts to keep both binaries installed and switch with a symlink:

```bash
# Install GitHub release binary into ~/.local/bin/acartine_knots/release/kno
scripts/release/channel-install.sh release

# Install local smoke-tested build into ~/.local/bin/acartine_knots/local/kno
scripts/release/channel-install.sh local

# Switch active ~/.local/bin/kno symlink
scripts/release/channel-use.sh release
scripts/release/channel-use.sh local

# Show current active target
scripts/release/channel-use.sh show
```

You can override defaults with:
- `KNOTS_CHANNEL_ROOT` (default: `~/.local/bin/acartine_knots`)
- `KNOTS_ACTIVE_LINK` (default: `~/.local/bin/kno`)
- `KNOTS_LEGACY_LINK` (default: `~/.local/bin/knots`)

Knots remains supported as a compatibility alias:
```bash
knots --version
```

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
