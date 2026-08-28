# Compaction redesign

## Active event-record compaction repair

- [x] Batch Git path operations so six-figure legacy histories cannot exceed the OS argument limit.
- [x] Make `compact --write-snapshots` activate a complete local generation and bounded active set.
- [x] Make sync publish and consume the activated generation without enumerating legacy history.
- [x] Retain raw recovery data outside active legacy scan roots and fence stale compatibility writers.
- [x] Preserve leases, interrupted activation, rollback, and exact-byte recovery fail closed.
- [x] Add integration and performance tests proving active file reduction and bounded steady-state sync.
- [x] Rebuild the release binary and prove compaction on at least 120,000 event/index files.
- [ ] Rebase onto current main, run `make sanity`, publish, and verify the exact remote SHA.

- [x] Prove current old-client behavior against a pruned generation.
- [x] Define the steady-state writer, checkpoint, and garbage-collection protocol.
- [x] Define a lossless v1 compatibility bridge and enforced retirement boundary.
- [x] Adversarially review multi-machine, offline-client, lease, and rollback behavior.
- [x] Re-scope the implementation Knots and approval points before resuming e2e work.
- [x] Approve the protected protocol-v2 Wave 1 implementation plan.
- [x] Implement the protected protocol-v2 Wave 1 contract.
- [x] Prove legacy v0.17.6 writes cannot alter canonical v2 state.
- [x] Run repository sanity gates.
- [ ] Ship through GitHub and verify the merged main SHA.

## knots-ccdc lossless generation integrator

- [x] Approve the data-only integrator and deterministic artifact plan.
- [x] Validate hostile inbox bundles and monotonic writer vectors.
- [x] Build deterministic lossless packs and exact event indexes.
- [x] Require projection-complete replay equivalence and durable conflicts.
- [x] Prove immutable-first publication, exact control CAS, and recovery epochs.
- [x] Run focused compaction tests and `make sanity`.
- [x] Publish the attributed branch for implementation review.

## Wave 2 durable outboxes

- [x] Add the local durable outbox, writer epoch, and legacy quarantine schema.
- [x] Record every App-authored event receipt before compatibility files and cache mutation.
- [x] Publish only pending receipts to a credential-scoped inbox with exact ref leases.
- [x] Confirm remote inbox OIDs before acknowledgement and recover lost acknowledgements.
- [x] Prove writer rotation, two-clone isolation, and legacy retention with focused tests.
- [x] Run the released v0.17.6 fixture and full repository sanity gates.
- [ ] Publish through GitHub and verify merged main, CI, attribution, and terminal Knot state.

## Wave 3 checkpoint bootstrap

- [x] Start from protected-v2 and durable-outbox main with the withdrawn v1 work preserved.
- [x] Add the generation checkpoint, exact writer/event index, and generation quarantine schema.
- [x] Persist fingerprint-derived writer identities with crash-safe local signing-key rotation.
- [x] Validate protected control, manifest, snapshots, projections, packs, and source facts first.
- [x] Replace every SQLite projection and generation watermark in one transaction.
- [x] Preserve locally-authored outboxes and migrate leased knots to generation quarantine.
- [x] Prove fresh bootstrap, pre-cutoff replacement, crash rollback, corruption rejection, and replay equivalence.
- [x] Run `make sanity`, publish through GitHub, and verify exact merged main and terminal Knot state.

## Wave 3 signed GitHub outboxes

- [x] Define canonical Ed25519 submission transcripts and immutable proposal refs.
- [x] Build submissions from SQLite-backed fingerprint identities and durable outbox receipts.
- [x] Verify repository/ref/OID/sequence/key rotation/signature/payload bindings fail closed.
- [x] Remove direct client inbox publication from the transport capability.
- [x] Prove forgery, replay, cross-writer, crash recovery, and v0.17.6 rejection paths.
- [x] Run `make sanity`, independent review, GitHub CI/merge, and exact-main proof.

## Wave 3 trusted GitHub enforcement (superseded personal-repo draft)

- [x] Resume `knots-74e2` from signed-outbox main with a fresh attributed lease.
- [x] Wrap the shipped signed proposal verifier in a trusted GitHub adapter.
- [x] Keep proposal/inbox data untrusted and construct protected refs in trusted code.
- [x] Remove the unsupported Integration 15368 bypass design after live preflight.
- [ ] Prove disposable ordinary denial and trusted Actions success before production.
- [x] Run focused tests, `make sanity`, and an independent implementation review.
- [ ] Commit with Sneka attribution, open a GitHub PR, pass CI, and merge.
- [ ] Apply and read back production v2 rules while leaving legacy freeze disabled.
- [ ] Record live evidence and advance `knots-74e2` to `SHIPPED`.

## Wave 3 zero-bypass attested epochs correction

- [x] Prove the public personal repository has Actions, OIDC, and attestation support.
- [x] Replace mutable inbox/registry promotion with immutable create-only v2 refs.
- [x] Add an immutable-authority workflow that constructs and attests the control manifest.
- [x] Ship Phase A with an empty signer allowlist and protected authority-code policy.
- [x] Merge Phase A, protect a random authority-code ref, and capture its exact SHA.
- [x] Add Phase B with only the reviewed authority ref/SHA/workflow tuple allowlisted.
- [x] Verify repository, workflow, source ref/SHA, epoch monotonicity, and writer dominance.
- [ ] Reject unattested lookalikes and every rewrite or deletion without bypass actors.
- [ ] Update the downstream provider-integrator and activation handoffs.
- [x] Run focused tests, full sanity, and an independent security review.
- [ ] Commit with Sneka attribution, publish a follow-up PR, pass CI, and merge.
- [ ] Apply/read back zero-bypass rules and capture live canary evidence.
- [ ] Advance `knots-74e2` to `SHIPPED` with exact-main and attestation proof.

## knots-74e2 Phase D authority rotation

- [x] Protect and prove a random immutable authority-code ref at exact Phase C SHA.
- [x] Rotate the checked-in signer allowlist to only the Phase C ref/SHA/workflow tuple.
- [x] Prove the retained Phase A authority ref is no longer trusted.
- [x] Run focused policy tests, full sanity, and independent security review.
- [x] Publish Phase D through GitHub PR, CI, and exact-main merge proof.
- [ ] Run the live attestation canary and apply/read back production zero-bypass rules.
- [ ] Record downstream handoffs and advance `knots-74e2` to `SHIPPED`.

## knots-74e2 canary recovery

- [x] Diagnose the sandbox-only Sigstore cache denial without weakening verification.
- [x] Reset only the disposable canary namespace and restore its zero-bypass ruleset.
- [x] Fetch the exact published control commit before constructing rewrite probes.
- [x] Publish the driver fix and rerun the complete live canary from exact main.
- [x] Bind activation evidence to the canonical hyphen-delimited manifest digest.
- [ ] Apply and read back production rules while keeping legacy freeze disabled.
