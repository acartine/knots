---
name: ship-release
description: >-
  Ship a new Knots release by verifying release readiness on main, previewing
  pending changesets and commits, running required quality gates, merging or
  validating the Version Packages PR, and confirming the GitHub Release assets
  are published.
---

# /ship-release

Ship a new Knots release.

## Use this skill when

- The user asks to cut, ship, publish, or verify a new Knots release.
- The user wants help with the Changesets-driven version PR and release flow.
- The user needs recovery help when a version bump merged but the GitHub
  release or assets did not publish.

## Steps

1. **Confirm release state on `main`** — Work from a clean `main` branch and
   pull the latest remote state first. Check the current version in
   `Cargo.toml` and `package.json`, then compare it with the latest semver
   tag and the GitHub release for `v<version>`.
   Preview commits since the latest semver tag with:
   `git log "$(git tag --sort=-version:refname | head -n 1)"..HEAD --oneline`

2. **Inspect pending release inputs** — Check whether there are unreleased
   changesets in `.changeset/` and whether a `Version Packages` PR already
   exists or was recently merged. For local version consistency, run:
   `npm run check-cargo-version`

3. **Audit changeset coverage** — Walk every commit since the last tag
   (`git log "$(git tag --sort=-version:refname | head -n 1)"..HEAD --oneline`)
   and classify each one:
   - **User-facing** (CLI flags, commands, output format, defaults, errors,
     config, deprecations, bug fixes users would notice) → must be covered by
     a changeset in `.changeset/` or be folded into an existing changeset's
     body.
   - **Internal-only** (refactors, test-only changes, CI, docs, comments, dep
     bumps with no behavior change) → no changeset required.

   If any user-facing commit lacks a changeset during an explicit ship-release
   request, author the missing changesets, commit+push them, and let the
   Changesets workflow refresh the Version Packages PR. Do not stop merely to
   ask whether to add release-note coverage. Do not ship a release that
   silently omits user-facing changes from the changelog. Re-run this audit
   against the refreshed PR before continuing to step 4.

   **CHANGESET PACKAGE NAME INVARIANT: THE ONLY VALID PACKAGE NAME IS
   `"knots"`.**
   THIS IS NOT OPTIONAL. THIS IS NOT A BEST-EFFORT CHECK. NEVER WRITE
   `"kno": patch`, `"kno": minor`, or `"kno": major` in changeset
   frontmatter. `kno` is the CLI binary name only; it is NEVER a Changesets
   package name. If a changeset contains `"kno"` as the package key, fix it to
   `"knots"` before any commit, push, PR, or release step.

   Every changeset frontmatter block must use exactly one of:
   - `"knots": patch`
   - `"knots": minor`
   - `"knots": major`

4. **Decide the path** —
   - If user-facing changes are on `main` and no version PR exists yet, let the
     Changesets workflow create or update the `Version Packages` PR.
   - If the `Version Packages` PR exists, review the bump, changelog, and
     version sync before merging it.
   - If the version PR is already merged and a GitHub release already exists
     for `v<version>`, treat it as already shipped and verify assets instead of
     calling it a tag collision.
   - If the version PR is already merged and the GitHub release is missing or
     incomplete, move to release verification or recovery.

5. **Run required quality gates** — Before merging a version PR or retrying a
   release, run:
   - `make sanity`
   - `npm run check-cargo-version`

   If release tooling changed, also run:
   - `scripts/release/smoke-install.sh`

   Stop on any failure. Do not ship a release with failing sanity checks.

6. **Trigger or watch the release** —
   - Normal path: merge the `Version Packages` PR into `main`. The `Release`
     GitHub Actions workflow should trigger automatically on the push.
   - Recovery path: if the version bump is already on `main` but the release is
     missing or incomplete, inspect the `Release` workflow and re-run it or
     trigger it with `workflow_dispatch`.

7. **Verify published outputs** — Confirm the GitHub Release exists for
   `v<version>`, that its body contains the `CHANGELOG.md` section for that
   version rather than only the `Version Packages` PR, and that these assets
   are attached:
   - `knots-v<semver>-darwin-arm64.tar.gz`
   - `knots-v<semver>-linux-x86_64.tar.gz`
   - `knots-v<semver>-linux-aarch64.tar.gz`
   - `knots-v<semver>-checksums.txt`

   To preview the release body locally, run:
   `scripts/release/notes-from-changelog.sh <version> CHANGELOG.md`

8. **Report** — Tell the user the released version, whether the release was
   newly published or recovered, link the GitHub Release, and call out any
   follow-up work if verification found a problem. Include a one- or
   two-bullet summary from the published release notes so the user can see the
   release story without opening GitHub.

## Notes

- Knots uses Changesets to open a `Version Packages` PR; the release is not cut
  by manually bumping a `patch`, `minor`, or `major` flag in this repo.
- `npm run version-packages` runs `changeset version` and then syncs
  `Cargo.toml` and `package.json`.
- The release workflow publishes GitHub release notes from the Changesets-built
  `CHANGELOG.md` entry for the version. Do not rely on GitHub generated notes:
  the release commit is usually the `Version Packages` merge, so generated
  notes tend to omit the actual user-facing changes.
- An existing `v<version>` tag is normal after a successful release. Treat only
  `tag exists but release is missing` or `release exists but required assets are
  missing` as recovery conditions.
