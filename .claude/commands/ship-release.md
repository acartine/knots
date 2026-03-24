# Ship Release

Cut a new release from main by summarizing recent work, creating a changeset,
and shepherding the Version Packages PR through merge.

## Steps

### 1. Identify unreleased commits

Find the latest release tag (format `v*`) and list all commits on `main` since
that tag:

```
git fetch --tags
git log $(git describe --tags --abbrev=0 --match 'v*')..HEAD --oneline
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

### 7. Merge the Version Packages PR

Once the PR exists and CI is green:

```
gh pr merge <number> --squash --auto
```

Report the merged PR URL to the user.
