# Contributing

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
