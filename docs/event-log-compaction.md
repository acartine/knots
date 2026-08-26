# Event-Log Compaction Protocol

Status: protocol-v2 contract implemented; provider enforcement and cache projection are follow-on
work. The current `compact --write-snapshots` command does not activate protocol v2.

## Why a protected protocol is required

Released legacy clients scan local `.knots/events`, `.knots/index`, and `.knots/snapshots` before
fetch. They can republish any path removed from their remote ref. A client-side version check or a
manifest in those directories cannot prevent resurrection across offline machines.

Protocol v2 therefore uses two independent boundaries:

- authoritative content lives only under `.knots/v2`
- provider policy makes every protocol-v2 ref immutable after its first creation

Legacy paths remain available but are non-authoritative. Protocol v2 never deletes them, prior
generations, archives, raw packs, refs, or Git objects.

## Ref roles

The default GitHub-compatible layout is:

```text
refs/heads/knots                              legacy compatibility
refs/heads/knots-v2-control/epochs/<epoch>/<run>-<nonce>-<digest>
                                                attested immutable control epoch
refs/heads/knots-v2-canonical/<generation>/<nonce>
                                                immutable generation
refs/heads/knots-v2-archive/<generation>/<nonce>
                                                additive immutable archive
refs/heads/knots-v2-proposals/<writer>/<seq>/<oid>
                                                immutable untrusted submission
refs/heads/knots-v2-authority-code/<nonce>      immutable reviewed workflow code
```

The provider integration must enforce equivalent roles when it maps them to another ref namespace.
A configured ref name is not evidence that these protections exist.

## Provider-protection capability

Publication requires a private validated capability. The validator compares a tracked marker with
facts returned by the provider:

- repository identity
- policy/ruleset identity and SHA-256 digest
- exact control-epoch, canonical, archive, and proposal ref patterns
- zero bypass actors and create-only behavior on every active v2 ruleset
- GitHub OIDC attestation identity bound to the repository, trusted workflow, immutable authority
  ref, and reviewed SHA
- the latest verified control epoch and its dominating canonical writer vector

Neither Git config, environment variables, nor a tracked marker can self-attest. Missing, stale,
forged, or unenforced provider facts fail closed. Until provider integration supplies live facts,
the shipped direct-publication and local-deletion paths remain disabled.

The local cache exposes `SyncService::install_v2_checkpoint` as the authenticated apply boundary.
It accepts only a `CheckpointInput` containing `ValidatedProtection`, the protected control and
canonical bytes, exact source facts, and retained-delta replay. Provider adapters must fetch and
validate those inputs before calling it; the cache never treats plain Git metadata as protection.

## Signed GitHub submissions

GitHub clients create one immutable proposal ref per sequence. Each installation has an Ed25519 key
stored transactionally in its mode-0600 local SQLite database. Its `writer_id` is the lowercase
SHA-256 fingerprint of the public key, so a random UUID cannot claim another writer's identity.

The client first builds one immutable proposal commit per writer sequence. Its proposal ref includes
that exact OID, which is unavailable to competing Git receive sessions before creation. It then
signs the OID and full ref.
The domain-separated envelope binds that OID, the repository identity, proposal and target refs,
exact expected inbox OID, writer sequence, previous head, causal generation, public key, and
every event path, length, ID, and payload SHA-256. Rotated keys include the previous writer ID
and a second signature from that parent key. The proposal is untrusted until the provider
integrator verifies the complete signed chain against the canonical writer vector.

The provider integrator reads exact proposal commits as untrusted data and never checks out or
executes them. It rejects unknown fields, malformed keys, forged signatures, repository/ref
substitution, stale OIDs, skipped or replayed sequences, cross-writer keys, unapproved rotation,
undeclared objects, and payload changes. Registration and sequence derive from the signed chain and
the last attested canonical writer vector. There is no mutable inbox or writer registry.

GitHub rulesets make that boundary enforceable without any bypass actor. Any credential may create
a new unique v2 ref, but nobody, including Actions or an administrator, can update or delete it.
Every authority-created ref includes an unpredictable or unpublished suffix, so an ordinary writer
cannot permanently squat the selected name before creation. Lookalike refs remain untrusted data.
A control epoch is authoritative only when its exact manifest
digest has GitHub artifact provenance from `acartine/knots`, the trusted control-epoch workflow,
an immutable authority-code ref, and its exact reviewed SHA. Trust bootstraps in two phases: the
attester first ships with an empty fail-closed allowlist; only a later reviewed commit may add the
exact protected ref, SHA, and workflow tuple. The current provider policy trusts only authority ref
`refs/heads/knots-v2-authority-code/e869300032bde6e8257673a3d11ccd60`, reviewed SHA
`f41b6733d80f9253362d5aacb725a3050b473e08`, and workflow
`.github/workflows/knots-v2-control-epoch.yml`. The workflow verifies the previous attested
epoch and requires the new writer vector to strictly dominate it before creating and attesting a
new immutable epoch. The legacy ref ruleset stays disabled until the guarded bridge cutover.
The attester is reusable rather than directly dispatchable. The canary caller is fixed to the
canary namespace; the provider-integrator caller must validate the generation before invoking the
production namespace.

## Immutable generation manifest

Each generation is content-addressed from all canonical identity fields. The manifest records:

- protocol version and generation identifier
- predecessor generation and predecessor control epoch
- exact legacy cutoff commit plus event and index tree identities
- sorted registered writer heads with inbox ref, commit, and sequence
- active and cold snapshot paths, hashes, byte lengths, and record counts
- immutable raw pack paths, pack identifiers, hashes, and byte lengths
- an exact event index containing event ID, raw-content hash, byte offset, and byte length
- minimum reader and writer protocol versions

Every authoritative path must be beneath `.knots/v2`. Snapshot and pack hashes are verified against
their bytes. Each indexed event slice is hashed independently, so its original bytes are exactly
recoverable. Duplicate writer IDs, pack IDs, or event IDs are rejected.

Raw packs and all generation archives are retained indefinitely. Retention limits are deliberately
not part of protocol v2.

## Activation

Activation is immutable-object-first and attested-control-last:

1. Read and validate provider policy facts.
2. Build and validate the generation, snapshots, writer vector, packs, and exact event index.
3. Publish the generation-specific canonical ref create-only.
4. Publish the generation-specific archive ref create-only.
5. Verify the latest attested epoch and its canonical writer vector.
6. Publish a unique immutable control-epoch manifest for exactly the next epoch.
7. Obtain and verify GitHub OIDC artifact provenance over the exact manifest digest.

An interruption before step seven leaves retained immutable objects or an unattested lookalike, but
does not change canonical authority. Conflicting attestations for one epoch fail closed.

## Monotonic control and rollback

The control record contains:

- a monotonically increasing epoch
- the exact previous attested epoch
- active generation ID and commit
- the generation archive ref
- the complete acknowledged writer-head vector
- the protection-policy digest
- an activation or recovery action

Every new authority increments the epoch by exactly one and must strictly dominate the canonical
writer vector. Equal writer sequences retain their prior acknowledgement. Rollback never rewrites a
control ref. It publishes and attests a higher recovery epoch that selects a previously archived
generation, names the epoch it recovers from, and preserves the acknowledged writer vector.

## Local safety

Wave 1 does not delete local files. Selecting a ref called `knots-v2` is insufficient proof of
provider protection, so the old generation fence fails closed before any filesystem mutation.
Provider and local-cache follow-on work may validate and project canonical v2 state, but it must
continue retaining legacy paths and raw packs under this protocol contract.

## Released-client compatibility fixture

`tests/fixtures/releases/v0.17.6` contains byte-identical event and index files produced by the
released v0.17.6 binary during the stale-writer audit. Its provenance records the release commit and
official release-archive SHA-256 digests.

The integration test publishes those exact legacy bytes to the legacy ref and proves both the
canonical-v2 ref OID and `.knots/v2` tree hash remain unchanged:

```sh
cargo test --test v0176_compaction_compat
```

The fixture remains network-free and deterministic in CI while preserving the actual old-client
output that exposed the unsafe same-path design.

## Verification

Run the focused protocol checks and repository gate:

```sh
cargo test compaction
cargo test --test v0176_compaction_compat
make sanity
```
