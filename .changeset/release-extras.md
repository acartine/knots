---
"knots": patch
---

Improve show/claim output and fix edge cases

- Display the latest note and handoff capsule in `show` and `claim` output,
  with a hint to use `-v`/`--verbose` to see all entries
- Guard `claim`/`peek` on queue state instead of relying on happy-path traversal
- Tighten metadata hints in `show` and `claim`
- Detect user's actual shell when `$SHELL` is unset
- Refine implementation review guidance
