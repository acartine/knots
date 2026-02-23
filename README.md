# Knots

Knots is a local-first, git-backed issue tracker designed for fast local workflows with
append-only events and a SQLite cache.

## Why Knots
- Keep issue data out of normal code PR diffs.
- Work offline first and sync through git when needed.
- Keep `new`, `state`, `ls`, and `show` fast by reading local cache data.
- Import existing history from JSONL or Dolt (source-only import).

## Quickstart
### Prerequisites
- Rust (stable)
- Git
- SQLite is bundled through `rusqlite` (`bundled` feature enabled)

### Build from source
```bash
cargo build --release
./target/release/knots --help
```

### Run with a local repo + cache path
```bash
knots --repo-root . --db .knots/cache/state.sqlite ls
```

## Milestone status
- M0 Workspace/Tracking: complete
- M1 Local Event + Cache Core: complete
- M1.5 Import Ingestion: complete
- M2 Dedicated Branch Sync: complete
- M2.5 Public Repo Readiness: in progress
- M2.6 Release + Curl Install: in progress
- M3+ Tiering, concurrency, and operability: pending

## Install with curl
The installer pulls from GitHub Releases and installs to `${HOME}/.local/bin` by default.

Latest release:
```bash
curl -fsSL https://raw.githubusercontent.com/<owner>/<repo>/main/install.sh | sh
```

Pinned release:
```bash
curl -fsSL https://raw.githubusercontent.com/<owner>/<repo>/main/install.sh \
  | KNOTS_VERSION=v0.1.0 sh
```

Custom install directory:
```bash
curl -fsSL https://raw.githubusercontent.com/<owner>/<repo>/main/install.sh \
  | KNOTS_INSTALL_DIR="$HOME/.knots/bin" sh
```

Verify install:
```bash
knots --version
```

## Core usage
Create an issue:
```bash
knots new "Document release pipeline" --state work_item
```

Update state:
```bash
knots state <knot-id> implementing
```

List and inspect:
```bash
knots ls
knots show <knot-id>
```

Sync from dedicated `knots` branch/worktree:
```bash
knots sync
```

Manage dependency edges:
```bash
knots edge add <src-id> blocked_by <dst-id>
knots edge list <src-id> --direction outgoing
knots edge remove <src-id> blocked_by <dst-id>
```

## Import from Beads
Export Beads to JSONL, then import.

```bash
bd sync --flush-only
knots import jsonl --file .beads/issues.jsonl
knots import status
```

Optional Dolt source import:
```bash
knots import dolt --repo /path/to/dolt/repo
```

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

The script validates both latest and pinned install flows and confirms `knots.previous` is
retained after reinstall.

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
