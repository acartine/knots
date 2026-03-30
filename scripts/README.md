# scripts

Build automation, release tooling, and git hooks.

## scripts/repo/

- **`pre-push-sanity.sh`** — runs `make sanity` before every push
- **`install-hooks.sh`** — installs the managed pre-push hook
- **`check-file-sizes.sh`** — enforces < 500 lines per .rs file
- **`check-coverage-threshold.sh`** — prevents coverage regressions
- **`require-changeset.sh`** — ensures changesets are present for releases

## scripts/release/

- **`sync-cargo-version.mjs`** — sync version between Cargo.toml and package.json
- **`channel-install.sh`** — install from release channel
- **`smoke-install.sh`** — post-install smoke test
