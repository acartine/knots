# Compaction redesign

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

## Wave 3 trusted GitHub enforcement

- [x] Resume `knots-74e2` from signed-outbox main with a fresh attributed lease.
- [x] Wrap the shipped signed proposal verifier in a trusted GitHub adapter.
- [x] Keep proposal/inbox data untrusted and construct protected refs in trusted code.
- [x] Enforce Integration 15368 as the only Knots v2 bypass actor.
- [ ] Prove disposable ordinary denial and trusted Actions success before production.
- [x] Run focused tests, `make sanity`, and an independent implementation review.
- [ ] Commit with Sneka attribution, open a GitHub PR, pass CI, and merge.
- [ ] Apply and read back production v2 rules while leaving legacy freeze disabled.
- [ ] Record live evidence and advance `knots-74e2` to `SHIPPED`.
