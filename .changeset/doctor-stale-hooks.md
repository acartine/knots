---
"knots": patch
---

Fix doctor to detect and fix stale/orphaned hooks. check_hooks now warns on
outdated hook content and leftover legacy hooks (e.g. post-commit). doctor --fix
removes legacy hooks before reinstalling current managed hooks.
