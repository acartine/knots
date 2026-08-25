# Compaction approval ledger

Recorded from Andrew's 2026-08-25 approval for this compaction fix.

## Approved now

- GitHub rulesets, trusted Actions integration, canonical/inbox/archive refs, live policy probes,
  and repository-by-repository activation for Knots data.
- Diffinite receive-policy and capability changes, the integrator service identity and grant,
  deployment through the existing rail, and live policy probes.
- All required changes to the local `.knots/cache/state.sqlite`, including schema migration,
  metadata, transactional projection replacement, quarantine state, and local backfill.
- GitHub branches, pull requests, merges, releases, service credentials, canaries, Hermes
  pause/resume, and additive archive creation through their normal rails.

## Deliberately not required by this fix

- Deleting legacy refs, archive refs, prior generations, raw event packs, or Git objects.
- Server-side object garbage collection or a finite archive-retention decision.
- A production PostgreSQL schema migration, a new host, DNS, TLS, or network provisioning.
- A new GitHub App: the existing GitHub Actions integration is the planned trusted writer.

The fix must retain archives indefinitely and use the existing hosts and deployment rails. If an
implementation would cross one of these exclusions, redesign it within the approved scope instead
of pausing later for another approval.
