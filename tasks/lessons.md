# Lessons

- Never claim distributed upgrade enforcement from client-side checks. For Knots, trace old
  binary fetch, apply, and push behavior plus remote receive policy before designing destructive
  event compaction.
- Preflight whether a repository owner can admit a proposed ruleset bypass before merging policy.
- Prefer immutable create-only refs with cryptographic authority over persistent bypass credentials.
- Bootstrap workflow authority in two reviewed phases: ship an empty fail-closed allowlist, then
  protect a random immutable code ref before a later commit allowlists its exact ref and SHA.
- Never call protocol scaffolding event compaction. Compaction is complete only when production
  sync uses a bounded retained set and live proof shows history size no longer drives sync time.
- Rebase test-binary fixes onto current main before rebuilding, rerunning the large-history proof,
  and publishing the branch.
- Do not ship incidental sync fixes as the answer to a compaction request. The acceptance test must
  show fewer active records and history-independent sync before publication.
- Test realistic compaction volume early; archival compression must not dominate the migration.
