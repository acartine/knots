---
"knots": patch
---

Reclaim leaked temp workspaces. Test workspaces now come from a shared RAII
guard that removes the directory even when a test panics, and
`scripts/repo/reap-stale-artifacts.sh` reaps stale `knots-*` trees under the
temp root as well as build artifacts under `target/`. Also fixes a real leak in
the `kno perf` harness, which removed only its clone and left the enclosing
root — with `origin.git` inside it — behind on every run.
