---
"knots": patch
---

Fix `kno sync` writing to `.git/FETCH_HEAD` during its internal fetch of the
`knots` ref. That stale `FETCH_HEAD` entry could make a subsequent
`git pull --ff-only origin main` try to fast-forward to both `origin/main`
and `origin/knots` at once and fail with "Cannot fast-forward to multiple
branches". The sync fetch now passes `--no-write-fetch-head` so it no longer
interferes with the repo's own `FETCH_HEAD`.
