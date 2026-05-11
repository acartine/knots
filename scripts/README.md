# scripts

Build automation, release tooling, and git hooks.

## scripts/repo/

- **`pre-push-sanity.sh`** — runs `make coverage` before every push
- **`install-hooks.sh`** — installs the managed pre-push hook
- **`check-file-sizes.sh`** — enforces < 500 lines per .rs file
- **`check-coverage-threshold.sh`** — prevents coverage regressions
- **`require-changeset.sh`** — ensures changesets are present for releases
- **`publish-public.sh`** — publish release artifacts to the public channel

## scripts/release/

- **`check-changesets.mjs`** — validates changeset package keys before release
- **`notes-from-changelog.sh`** — extract GitHub Release notes from CHANGELOG.md
- **`sync-cargo-version.mjs`** — sync version between Cargo.toml and package.json
- **`channel-install.sh`** — install from a named release channel
- **`channel-use.sh`** — switch the active release channel
- **`smoke-install.sh`** — post-install smoke test
