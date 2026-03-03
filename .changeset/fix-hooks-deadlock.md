---
"knots": patch
---

### Fixes

- Remove `post-commit` from managed hooks to prevent recursive fork bomb
  where each sync commit spawned another background `kno sync`.
- Change hook template from backgrounded `kno sync` to foreground `kno pull`
  so errors are visible.
- Add `--no-verify` to internal sync commits to prevent hook recursion while
  locks are held.
