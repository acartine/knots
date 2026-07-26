---
"knots": patch
---

Add `kno rollback <id> --outcome <name>` for workflow-declared failure
transitions. A failed action can now land on the target its prompt declares —
shipment's `merge_conflicts` goes straight to `ready_for_implementation` —
instead of needing repeated generic rollbacks that queue states correctly
reject. The move, the failed step-history entry, and the lease release
(terminate plus unbind) happen as one atomic operation, so a failed action no
longer leaves the knot claimed or requires manual lease surgery.

`kno rollback` also accepts `--lease <id>` now, matching what the managed
skills already documented. Bare `kno rollback` is unchanged: it still rewinds
to the immediately preceding queue state and still rejects queue states.

Undeclared outcome names, success outcomes, and arbitrary target states are
rejected without touching state or lease, so this stays a constrained
transition rather than a force-state escape hatch.

Also restores the `## Failure Modes` section to the compiled `work_sdlc`
prompts, which had drifted out of `dist/bundle.json`, and adds a parity gate
so source and compiled prompts cannot silently diverge again.
