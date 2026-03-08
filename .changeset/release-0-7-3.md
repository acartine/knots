---
"knots": patch
---

- Detect stale lock files via PID and reduce lock timeout from 30s to 5s
- Resolve latest version via redirect instead of GitHub API to avoid rate limits
- Auto-add install directory to PATH in shell rc file
