# Event-Log Compaction Protocol

Status: design only. The current `compact --write-snapshots` command does not prune safely.

## Decision

Knots should compact by publishing a versioned checkpoint into a generation-two replication
namespace. It should retain a bounded delta after that checkpoint and prune only exact files
represented by the checkpoint.

Generation two is a prerequisite, not an optimization. A legacy client scans every local event
and can republish files deleted from its remote branch. A separate remote ref prevents that client
from resurrecting files in the active generation-two ref.

The first implementation should not rewrite Git history. Deletion commits and a new active ref
bound checkout size and sync scans without invalidating existing commits. Old blobs remain in Git
history, so this choice does not reclaim all repository bytes.

## Goals And Non-Goals

The protocol must:

- bound JSON files in the active checkout independently of lifetime event count
- preserve immutable-event conflict detection
- reconstruct a fresh cache from a checkpoint plus a retained delta
- migrate stores whose watermarks or quarantine bases predate the cutoff
- preserve events written concurrently with compaction
- reject incomplete, corrupt, stale, or unsupported generations
- recover without an online coordinator

The first version does not:

- shrink all reachable Git objects
- accept writes from clients that only understand the legacy ref
- delete a path merely because its timestamp is old
- treat a snapshot filename as proof that a safe checkpoint exists

## Measured Baseline

The checked-in fixture and inventory command model 80,000 event and index files. The recorded run
used Node 24.14.1 on Linux 6.12.101 x86-64 and a `tmpfs` filesystem at
2026-08-25T08:18:45Z. Reproduce it with:

```sh
make event-log-fixture EVENT_LOG_ROOT=/tmp/knots-event-log-80k
make event-log-inventory EVENT_LOG_ROOT=/tmp/knots-event-log-80k \
  EVENT_LOG_INVENTORY_ARGS="--runs 5"
```

| Measurement | Baseline | Post-compaction target |
|---|---:|---:|
| Full-event files | 50,000 | At most 1,000 retained |
| Index-event files | 30,000 | At most 1,000 retained |
| Snapshot files | 0 | 2 |
| Manifest files | 0 | 1 |
| Total JSON files | 80,000 | 2,003 |
| Full-event bytes | 9,238,894 | At most baseline bytes |
| Index-event bytes | 8,958,894 | At most baseline bytes |
| Snapshot bytes | 0 | Reported by implementation |
| Daily full-event rate | 1,250 over 40 days | Informational |
| Daily index-event rate | 750 over 40 days | Informational |
| Median no-op scan | 533.977 ms over 5 runs | At most 133.494 ms |
| Median no-op sync model | 1,581.668 ms over 5 runs | At most 395.417 ms |
| No-op sync ratio | 100 percent | At most 25 percent of baseline |

The sync model repeats the production hot path that dominates a no-op push: recursively collect
all JSON files, read every source blob, and compare it with a prebuilt byte mirror. It excludes
network fetch, Git reset, and the unchanged-watermark pull. Those costs depend on the remote and
must be reported separately by implementation knot `246b`; this deterministic result is the local
file-work baseline and a conservative target denominator.

An earlier Quilt inventory observed 50,921 full events and 32,754 index events. Those files used
about 201 MiB and 128 MiB respectively. The store had no snapshots and pushed by scanning 83,675
files. Recent full-event churn was commonly 1,000 to 2,500 files per day, with a 4,834-file peak.
These observations motivate the fixture but do not replace its reproducible measurements.

## Why The Current Path Is Unbounded

`src/replication.rs` calls `collect_local_event_files` before each push attempt. The function walks
all JSON beneath `index`, `events`, and `snapshots`. It then compares and copies that complete list
into the publish worktree. A no-op push is therefore O(total historical files).

`src/snapshots.rs` writes independent active and cold files with ordinary file writes. Loading
chooses the lexicographically latest file of each kind independently. It does not bind the pair,
verify hashes, reject an unsupported `schema_version`, or apply the pair in one database
transaction.

`src/sync/apply.rs` loads those snapshots only when both Git watermark keys are absent. It then
scans or diffs all event files and advances both watermarks to the target head. Existing stores do
not use snapshots to cross a cutoff.

`src/sync/apply_quarantine.rs` pins each held knot to the watermark from its first skipped pull.
The shared watermarks continue advancing. When a lease ends, the applier diffs the pinned commit
to the target and removes the quarantine row after replay.

Snapshot creation alone is unsafe retention. A stale store still has every old local file, and
`collect_local_event_files` copies missing files back into the remote checkout. Pruned files are
therefore resurrected on its next push.

Deletion also breaks current quarantine replay. Git reports a deleted path in the diff, but the
applier ignores it because the file no longer exists. It can then remove the quarantine row without
ever applying the held-back event.

## Replication Generations

Generation one is the configured legacy ref, normally `refs/work/knots` on Diffinite or
`refs/heads/knots` elsewhere. Generation two uses a distinct configured ref, proposed as
`refs/work/knots-v2` or `refs/heads/knots-v2`.

The migration rail freezes generation one at a declared commit. Generation-two clients read and
write only generation two after activation. A legacy client can modify only generation one, so it
cannot add files to generation two or resurrect its compacted paths.

The old ref remains available for rollback and old read-only clients. Writes made there after the
freeze are unsupported and do not silently merge into generation two. Operators must upgrade a
writer before its work is authoritative.

This isolation is the only coordinator-free mixed-version rule that current clients can satisfy.
A manifest inside the old tree is insufficient because an old binary does not understand it.

## Manifest And Checkpoint Schema

Each immutable generation manifest is stored at:

```text
.knots/compaction/generations/<generation-id>/manifest.json
```

The active generation-two ref contains exactly one active manifest. The manifest has this logical
schema; field names are normative, while the Rust representation belongs to follow-on work.

```json
{
  "protocol_version": 2,
  "generation_id": "g2-<sha256>",
  "state": "active",
  "source": {
    "remote_ref": "refs/work/knots",
    "cutoff_commit": "<40-or-64-character-object-id>",
    "index_tree": "<git-tree-object-id>",
    "event_tree": "<git-tree-object-id>"
  },
  "snapshots": {
    "active": {
      "path": ".knots/snapshots/<id>-active_catalog.snapshot.json",
      "sha256": "<hex>",
      "bytes": 0,
      "records": 0
    },
    "cold": {
      "path": ".knots/snapshots/<id>-cold_catalog.snapshot.json",
      "sha256": "<hex>",
      "bytes": 0,
      "records": 0
    }
  },
  "retention": {
    "max_full_files": 1000,
    "max_index_files": 1000
  },
  "compatibility": {
    "minimum_reader_protocol": 2,
    "minimum_writer_protocol": 2
  },
  "previous_generation": null
}
```

The generation identifier is the SHA-256 digest of canonical manifest identity fields and both
snapshot digests. Informational timestamps must not affect identity. The manifest is invalid if:

- its protocol or state is unsupported
- its identifier does not match its canonical content
- either snapshot is absent, outside `.knots/snapshots`, or has the wrong digest or length
- its source commit or named trees cannot be resolved
- its cutoff is not an ancestor of the prepared generation
- counts disagree with decoded snapshots
- a previous-generation link forms a cycle or skips the active predecessor

The active and cold snapshots form one indivisible checkpoint. A reader must never choose either
file by filename order.

## Exact Cutoff Rule

The cutoff set is the union of JSON paths in the `index_tree` and `event_tree` named by the
manifest. Membership is based on Git tree identity at `cutoff_commit`, not filename date,
`occurred_at`, UUID order, or wall clock.

A path can be pruned only when all these conditions hold:

1. The checkpoint was materialized from the exact cutoff commit.
2. The path and its blob identity occur in the named cutoff tree.
3. The active and cold snapshot hashes have been verified.
4. The path is outside the chosen retained delta.
5. No conflicting local blob exists at the same immutable path.

An event created after the cutoff remains eligible even when clock skew places it in an old date
directory. An unpushed local path absent from the cutoff tree also remains eligible. A differing
blob at a cutoff path is an immutable-file conflict and must stop publication.

The retained delta is deterministic. Starting from the files after the checkpoint boundary, keep
the newest 1,000 full-event paths and newest 1,000 index-event paths in canonical event order. The
manifest records the limits. Tests must use a deterministic tie-breaker on relative path.

## Two-Phase Publication

Publication uses Git compare-and-swap ref updates as its only serialization mechanism.

### Phase One: Prepare

1. Fetch the source ref and record its exact head as cutoff commit `C`.
2. Materialize an empty database from `C`; do not snapshot a possibly stale local cache.
3. Write both snapshots to temporary files, flush them, and atomically rename them.
4. Compute hashes, counts, tree identities, and the canonical generation identifier.
5. Build a prepared commit containing the snapshots, manifest, and bounded retained delta.
6. Publish it to a generation-specific staging ref with create-only compare-and-swap.
7. Reload the staged commit and verify the safety oracle before activation.

The source ref remains authoritative throughout preparation. A crash leaves either unreferenced
temporary files or an inactive staging ref. Neither is read by normal sync.

### Phase Two: Activate

1. Fetch both refs again.
2. Import every eligible event added after `C` into the prepared tree.
3. Rebuild or extend the retained delta without changing the checkpoint at `C`.
4. Verify that `C` is an ancestor and that every post-`C` eligible path is present.
5. Change the manifest state to `active` and rerun the complete validator.
6. Compare-and-swap the generation-two active ref from its expected old value to this commit.
7. If either source or active head moved, retry delta import or prepare a new generation.

The active ref update is the commit point. Before it, readers use the previous generation. After
it, readers see a complete checkpoint and delta together. No reader observes a published manifest
whose snapshots are absent.

The migration rail then freezes generation one. Subsequent generation-two writers fetch and reset
to the active head before scanning local files. They validate its manifest before copying anything.

## Local Pruning And Stale Writers

Local pruning happens only after activation is fetched back and validated. It must never be part of
the remote commit transaction.

For each local event or index file:

- preserve it when its path is absent from the cutoff trees
- preserve it when it belongs to the retained delta
- delete it when path and blob both match a compacted cutoff entry
- fail on a matching path with different bytes

Use atomic directory bookkeeping so an interrupted local prune can resume. Record the last fully
validated generation in SQLite. A later push repeats pruning idempotently before collecting files.

A stale generation-two writer must first fetch the current active manifest. It may publish its
unique local events after applying current cutoff filtering. It must not use its old watermark or
old manifest to decide which files to copy.

A generation-one binary cannot push to the generation-two ref because it has a different configured
remote ref. Server-side ref policy should additionally deny accidental direct writes to staging
refs and permit active-ref updates only through the compaction rail.

## Bootstrap And Incremental Pull

Checkpoint application must be one SQLite transaction with replace semantics. The current
upsert-only loader is not sufficient because rows absent from a checkpoint could survive, and one
tier can retain a duplicate moved to another tier.

### Fresh bootstrap

1. Resolve and validate the active manifest and both snapshots.
2. Replace hot, warm, and cold materializations from the checkpoint in one transaction.
3. Replay the retained remote delta after the checkpoint boundary.
4. Set both shared watermarks and the local generation marker only after replay succeeds.

### Existing watermark before the cutoff

Do not diff the old watermark through deleted paths. Rebase the cache onto the checkpoint, then
replay the union of retained remote events and eligible local unpushed events. Advance watermarks
only after the transaction and replay succeed.

### Existing watermark at or after the cutoff

When the local generation matches, use the normal Git diff. If its base commit is unreachable,
validate the checkpoint and use checkpoint rebasing instead of the current blind full-file scan.

### Lease quarantine

A quarantined row whose base predates the cutoff must gain a checkpoint generation and per-knot
baseline. While the lease remains local, checkpoint replacement must not overwrite that knot's
local materialization.

When the lease ends, restore the knot's verified checkpoint baseline, replay its eligible local and
remote events after the cutoff, and remove quarantine only after success. A missing or corrupt
baseline leaves the quarantine row and shared watermarks unchanged.

The applier should filter quarantine replay to the draining knot. Current replay walks the entire
diff once per quarantine row and relies on event idempotence.

## Concurrency Invariants

- A checkpoint always describes one immutable source commit, never a moving branch.
- A source event after the cutoff is retained and replayed, not folded into the old snapshot.
- Only one compare-and-swap activation wins for an expected active head.
- A losing compactor discards or revalidates its staging generation.
- A normal writer fetches the active generation before filtering local paths.
- A unique unpushed event absent from the cutoff tree is never deleted.
- An immutable path with different content is a conflict, never last-writer-wins.
- No lock held on one machine is treated as global coordination.

## Recovery And Rollback

| Failure point | Required behavior |
|---|---|
| Before snapshot rename | Remove or ignore temporary files; source stays authoritative. |
| One snapshot written | No active manifest references it; ignore it. |
| Prepared staging ref published | Validate or delete staging later; active readers ignore it. |
| Source advances during prepare | Import its delta and retry compare-and-swap. |
| Active compare-and-swap loses | Keep current active ref; rebase or discard the loser. |
| Active remote, local files unpruned | Pull validates; push resumes pruning before scanning. |
| Snapshot hash or schema failure | Abort without cache replacement or watermark movement. |
| Quarantine migration failure | Keep the row and its original base; do not expose remote state. |

Rollback moves the active generation-two ref, with compare-and-swap, to the last verified active
generation. Because Git history is retained, operators can also restore the legacy ref and switch
clients back through configuration. Rollback never selects an orphan snapshot by timestamp.

After rollback, writers must use the restored manifest's cutoff. Local stores whose generation is
newer must rebase from the restored checkpoint before publishing.

## Mixed-Version Rollout

The rollout has an unavoidable compatibility boundary:

1. Ship readers and writers that understand protocol two and the new ref.
2. Create and validate generation two while generation one remains authoritative.
3. Stop supported writers from using generation one.
4. Activate generation two atomically.
5. Freeze generation one and direct upgraded clients to generation two.
6. Keep generation one for rollback until the rollout window closes.

Old readers keep a frozen, consistent view. Old writers are unsupported after the freeze; their
commits remain isolated on generation one and cannot resurrect generation-two history. Supporting
arbitrary old writes without a server-side compatibility gate would sacrifice either correctness or
bounded retention.

## Git History And Repository Size

Do not rewrite history for the first implementation. The active checkout falls from lifetime files
to the manifest, two snapshots, and the bounded delta. Push scans and ordinary sync therefore become
bounded.

Deletion commits do not remove reachable blobs from Git packs. Keeping generation one for rollback
also keeps its full history reachable. Clone and remote storage size will not fall in proportion to
the checkout file count.

A later maintenance operation may create a root checkpoint commit and retire old refs after the
compatibility window. That is a separate, explicitly destructive protocol. It must migrate every
watermark and quarantine base because the old commits become unreachable. It is not required to
meet the file-count or no-op-sync targets here.

## Executable Safety Matrix

The checked-in protocol oracle should model these cases without depending on wall clock timing.

| Scenario | Invariant |
|---|---|
| Fresh bootstrap | Checkpoint plus retained delta equals full replay. |
| Watermark before cutoff | Rebase converges; deleted paths are never silently skipped. |
| Quarantined local knot | Local state remains held; unlock converges before row removal. |
| Concurrent writer | Every unique post-cutoff path survives activation. |
| Stale writer | Cutoff files are suppressed; unique unpushed files survive. |
| Legacy writer | It cannot change the generation-two ref. |
| Interrupted prepare | No incomplete generation becomes active. |
| Corrupt snapshot | Validation fails without cache or watermark changes. |
| Compactor race | Exactly one expected-head activation wins. |
| Rollback | Previous verified checkpoint becomes authoritative. |
| Second no-op sync | Zero changed files; target timing is met. |

The fixture benchmark must assert 2,003 JSON files after compaction and a median no-op sync no
greater than one second or 25 percent of baseline, whichever is stricter.

## Implementation Breakdown

The implementation is aggregate effort level 4 because it crosses snapshot schema, remote-ref
publication, push filtering, pull watermarks, quarantine, compatibility, and recovery. The four
independently shippable leaves total six effort points; effort levels are not additive calendar
estimates.

| Rank | Knot | Effort | Deliverable | Dependencies |
|---:|---|---:|---|---|
| 1 | `5807` | 1 | Manifest, checkpoint schema, validator, and protocol model | None |
| 2 | `5728` | 2 | Generation-two publication, push fencing, and local pruning | `5807` |
| 2 | `2af8` | 2 | Bootstrap, watermark rebasing, and quarantine migration | `5807` |
| 3 | `246b` | 1 | 80k benchmark, end-to-end tests, rollout checks, and docs | `5728`, `2af8` |

The principal risks are the mixed-version cutover, preserving unique unpushed events, and migrating
locally leased knots without exposing remote state. The generation-two boundary contains the first
risk. Exact Git-tree membership and the checkpoint-aware quarantine baseline address the others.

## Acceptance Targets

- The inventory reports counts, bytes, daily rate, and repeated no-op timings.
- The active generation contains exactly 2,003 JSON files in the standard fixture.
- Median no-op sync is at most one second and at most 25 percent of baseline.
- Fresh bootstrap and pre-cutoff incremental stores converge to full replay.
- Lease quarantine, stale writers, concurrent writers, and interruption tests pass.
- Unsupported manifests and mixed-version writes fail before state or watermarks change.
- `make sanity` and the source-size checks pass without lowering coverage.
