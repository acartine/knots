# Lessons

- Never claim distributed upgrade enforcement from client-side checks. For Knots, trace old
  binary fetch, apply, and push behavior plus remote receive policy before designing destructive
  event compaction.
- Preflight whether a repository owner can admit a proposed ruleset bypass before merging policy.
- Prefer immutable create-only refs with cryptographic authority over persistent bypass credentials.
- Bootstrap workflow authority in two reviewed phases: ship an empty fail-closed allowlist, then
  protect a random immutable code ref before a later commit allowlists its exact ref and SHA.
