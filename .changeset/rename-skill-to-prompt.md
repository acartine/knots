---
"knots": patch
---

Rename `kno skill <knot-id>` to `kno prompt <knot-id>` to remove the naming
collision with `kno skills` (the managed-skill installer). The old form still
works as a hidden alias for backward compatibility, but emits
`warning: 'kno skill' is deprecated; use 'kno prompt'` on stderr.
