# Event-Log Compaction Protocol

Status: protocol-v2 contract implemented; provider enforcement and cache projection are follow-on
work. The current `compact --write-snapshots` command does not activate protocol v2.

## Why a protected protocol is required

Released legacy clients scan local `.knots/events`, `.knots/index`, and `.knots/snapshots` before
fetch. They can republish any path removed from their remote ref. A client-side version check or a
manifest in those directories cannot prevent resurrection across offline machines.

Protocol v2 therefore uses two independent boundaries:

- authoritative content lives only under `.knots/v2`
- provider policy controls which credentials may update each protocol-v2 ref

Legacy paths remain available but are non-authoritative. Protocol v2 never deletes them, prior
generations, archives, raw packs, refs, or Git objects.

## Ref roles

The default GitHub-compatible layout is:

```text
refs/heads/knots                              legacy compatibility
refs/heads/knots-v2-control                   protected singleton control
refs/heads/knots-v2-canonical/<generation>    immutable generation
refs/heads/knots-v2-archive/<generation>      additive immutable archive
refs/heads/knots-v2-proposals/<writer>/<seq>  immutable untrusted submission
refs/heads/knots-v2-inbox/<writer-id>         protected Actions-owned writer inbox
```

The provider integration must enforce equivalent roles when it maps them to another ref namespace.
A configured ref name is not evidence that these protections exist.

## Provider-protection capability

Publication requires a private validated capability. The validator compares a tracked marker with
facts returned by the provider:

- repository identity
- policy/ruleset identity and SHA-256 digest
- exact control, canonical, archive, and inbox ref patterns
- trusted integrator identity
- observed control head
- protected control, create-only canonical/archive, and writer-scoped inbox behavior

Neither Git config, environment variables, nor a tracked marker can self-attest. Missing, stale,
forged, or unenforced provider facts fail closed. Until provider integration supplies live facts,
the shipped direct-publication and local-deletion paths remain disabled.

The local cache exposes `SyncService::install_v2_checkpoint` as the authenticated apply boundary.
It accepts only a `CheckpointInput` containing `ValidatedProtection`, the protected control and
canonical bytes, exact source facts, and retained-delta replay. Provider adapters must fetch and
validate those inputs before calling it; the cache never treats plain Git metadata as protection.

## Signed GitHub submissions

GitHub clients never create or update protected inbox refs. Each installation has an Ed25519 key
stored transactionally in its mode-0600 local SQLite database. Its `writer_id` is the lowercase
SHA-256 fingerprint of the public key, so a random UUID cannot claim another writer's identity.

The client first builds one immutable proposal commit per writer sequence, then signs its exact OID.
The domain-separated envelope binds that OID, the repository identity, proposal and target refs,
exact expected inbox OID, writer sequence, previous head, causal generation, public key, and every
event path, length, ID, and payload SHA-256. Rotated keys include the previous writer ID and a second
signature from that parent key. The proposal ref and signed envelope are only inputs to the trusted
Action; neither gives the client permission to update a protected inbox ref.

A trusted default-branch Action reads the exact proposal commit as untrusted data and never checks
out or executes it. The verifier rejects unknown fields, malformed keys, forged signatures,
repository/ref substitution, stale OIDs, skipped or replayed sequences, cross-writer keys,
unapproved rotation, undeclared objects, and payload changes. Only a successful verification
returns a `PromotionRequest`; the Action advances the protected inbox with the signed expected OID
as its compare-and-swap lease. Lost responses are recovered by polling that exact inbox OID.

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

Activation is immutable-object-first and control-CAS-last:

1. Read and validate provider policy facts.
2. Build and validate the generation, snapshots, writer vector, packs, and exact event index.
3. Publish the generation-specific canonical ref create-only.
4. Publish the generation-specific archive ref create-only.
5. Re-read the provider control head.
6. Push the new control record with an exact expected-head compare-and-swap.

An interruption before step six leaves retained immutable objects but does not change canonical
authority. A stale publisher loses the final compare-and-swap.

## Monotonic control and rollback

The control record contains:

- a monotonically increasing epoch
- the exact previous control head
- active generation ID and commit
- the generation archive ref
- the complete acknowledged writer-head vector
- the protection-policy digest
- an activation or recovery action

Every update increments the epoch by exactly one and must dominate all acknowledged writer heads.
Equal writer sequences must retain the same commit. Rollback never rewinds the control ref. It
publishes a higher recovery epoch that selects a previously archived generation, names the epoch it
recovers from, and preserves the acknowledged writer vector.

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
