# Merge Conflict Auto Resolution

This document describes the current algorithm for handling push/pull conflicts in Knots.

## Scope

The algorithm is implemented for the `kno push` and `kno sync` paths.

- `kno push` runs the publish flow.
- `kno sync` runs `push` first, then `pull`.

Code:
- [`ReplicationService::push`](src/replication.rs#L43)
- [`ReplicationService::sync`](src/replication.rs#L105)

## High-level strategy

Knots does not do line-based merge conflict resolution on event files.

Instead, it uses a retry-and-rebase loop around a dedicated worktree branch:

1. Move the worktree to latest remote `origin/knots` (or local HEAD fallback).
2. Copy local event files into the worktree.
3. Refuse to overwrite any existing remote file with different bytes.
4. Commit and push.
5. If push is rejected as non-fast-forward, retry from step 1.
6. If retries are exhausted, escalate with a conflict error.

## Detailed algorithm

### 1. Prepare worktree

Ensure the dedicated knots worktree exists and is on the knots branch.

Code:
- [`KnotsWorktree::ensure_exists`](src/sync/worktree.rs#L34)
- call site in push flow: [`ReplicationService::push`](src/replication.rs#L46)

### 2. Collect local event files

Read all local JSON files under:

- `.knots/index`
- `.knots/events`

Code:
- [`ReplicationService::collect_local_event_files`](src/replication.rs#L131)

### 3. Retry loop (`MAX_ATTEMPTS = 3`)

For each attempt:

1. Fetch and reset worktree to remote branch head.
   - If remote branch is missing/unknown, reset to local worktree HEAD.
2. Verify worktree is clean.
3. Copy local event files into worktree.

Code:
- loop and attempt budget: [`ReplicationService::push`](src/replication.rs#L52)
- fetch/reset logic: [`reset_worktree_to_remote_or_local`](src/replication.rs#L111)
- clean check call: [`worktree.ensure_clean`](src/replication.rs#L54)

### 4. File collision policy

When copying a local file into worktree:

- If destination file does not exist: copy it.
- If destination exists and bytes are identical: skip (idempotent).
- If destination exists and bytes differ: fail immediately with `FileConflict`.

Code:
- [`copy_files_into_worktree`](src/replication.rs#L165)
- emitted error: [`SyncError::FileConflict`](src/sync/mod.rs#L85)

This is the primary guard against silent data corruption.

### 5. Stage, commit, push

- Force-stage `.knots/index` and `.knots/events` (`git add -f`) so `.gitignore` does not
  block the publish branch flow.
- If nothing is staged, return no-op (`committed=false`, `pushed=false`).
- Otherwise commit and push.

Code:
- stage call: [`GitAdapter::add_paths`](src/sync/git.rs#L138)
- staged-change check: [`GitAdapter::has_staged_changes`](src/sync/git.rs#L147)
- commit: [`GitAdapter::commit`](src/sync/git.rs#L172)
- push: [`GitAdapter::push_branch`](src/sync/git.rs#L185)

### 6. Non-fast-forward handling

If push fails with a non-fast-forward style rejection:

- retry from a fresh fetch/reset, up to `MAX_ATTEMPTS`.
- after final attempt, escalate as `MergeConflictEscalation`.

Code:
- detection helper: [`SyncError::is_non_fast_forward`](src/sync/mod.rs#L115)
- retry/escalation branch: [`ReplicationService::push`](src/replication.rs#L90)
- emitted error: [`SyncError::MergeConflictEscalation`](src/sync/mod.rs#L88)

## What `kno sync` does with conflicts

`kno sync` is strict ordering:

1. `push`
2. `pull`

If `push` escalates conflict, `pull` is not run and the command returns an error.

Code:
- [`ReplicationService::sync`](src/replication.rs#L105)

## Test coverage

The replication flow is exercised in:

- [`replication::tests`](src/replication/tests.rs)

Notably:

- publish from dev1 clone,
- pull into dev2 clone,
- verify propagated knot state in cache.
