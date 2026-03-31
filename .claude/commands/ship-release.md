# Ship Release

Cut a new release from main by summarizing recent work, creating a changeset,
and shepherding the Version Packages PR through merge.

## Steps

### 1. Identify unreleased commits

Find the latest release tag (format `v*`) and list all commits on `main` since
that tag. Use semver sorting — **not** `git describe`, which returns the nearest
ancestor by commit distance and can return a lower version if tags exist
out of semver order in history.

```
git fetch --tags
latest_tag=$(git tag --list 'v*' --sort=-version:refname | head -1)
echo "Latest tag: $latest_tag"
git log ${latest_tag}..HEAD --oneline
```

If there are no new commits, stop and tell the user there is nothing to release.

### 2. Summarize changes

Read the diffs and commit messages. Classify each change as one of:
- **feature** – new user-facing capability
- **fix** – bug fix
- **chore** – internal cleanup, CI, docs, deps

Write a concise, bullet-pointed summary of the changes suitable for a
CHANGELOG entry.

### 3. Determine release type

Apply semver rules to decide the bump level:
| Condition | Bump |
|---|---|
| Breaking / incompatible API change | `major` |
| New feature or meaningful enhancement | `minor` |
| Bug fixes, chores, docs only | `patch` |

Briefly note the summary and bump level, then proceed autonomously — do not
wait for user confirmation.

### 4. Review existing changesets and fill gaps

List any existing `.changeset/*.md` files (excluding `config.json` and
`README.md`). These represent work already documented by contributors during
development. Read them and compare against the commit summary from step 2.

- If every commit is already covered by an existing changeset, skip to step 5
  — no new changeset is needed.
- If some commits are **not** covered by an existing changeset, create a new
  changeset file for the missing work. Use a short kebab-case filename
  (e.g., `release-extras.md`):

```
---
"knots": <patch|minor|major>
---

<Summary of changes not already covered by existing changesets>
```

The bump level in the new file should reflect only the uncovered changes.
The changesets tooling will pick the highest bump across all files
automatically.

**Do not delete existing changeset files.** The `changeset version` step
(run by the Version Packages workflow) consolidates all `.changeset/*.md`
files into `CHANGELOG.md` and removes them.

### 5. Commit and push

```
git add .changeset/
git commit -m "chore: add changeset for next release"
git push origin main
```

### 6. Wait for the Version Packages PR

The `changesets-version-pr` workflow will create or update a PR titled
**"Version Packages"**. Poll with:

```
gh pr list --search "Version Packages" --state open --json number,title
```

Wait until the PR appears (check every 30 seconds, up to 5 minutes).

### 7. Verify the planned version tag does not already exist

Before merging, read the version from the Version Packages PR branch to confirm
the planned tag is free:

```
gh pr checkout <number>
planned_version=$(grep '^version' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
planned_tag="v${planned_version}"
echo "Planned tag: $planned_tag"
if git ls-remote --exit-code --tags origin "refs/tags/${planned_tag}" >/dev/null 2>&1; then
  echo "ERROR: tag ${planned_tag} already exists on remote — do not merge, the release workflow will silently skip publishing"
  exit 1
fi
git checkout main
```

If the tag already exists, **stop and report the collision to the user** rather
than merging. The release workflow will succeed without publishing anything,
which is a silent failure.

### 8. Merge the Version Packages PR

Once the tag check passes and CI is green:

```
gh pr merge <number> --squash --auto
```

Report the merged PR URL to the user.
