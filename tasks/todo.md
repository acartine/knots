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
