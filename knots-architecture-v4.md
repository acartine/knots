# Knots — Architecture Design (Rust)

This document describes a robust, git-backed issue tracker named **Knots**, designed to feel “database-like” (fast queries, reliable concurrency) while remaining **repo-native** and **PR-invisible**.

---

## 1. Goals and Constraints

### Goals
- **No issue noise in normal PRs**: knots data must not appear in code PR diffs.
- **Distributed by git**: no always-on server required; works offline; syncs when online.
- **Robust with multiple clients on the same host**: a TUI + multiple shells should not corrupt state or crash.
- **Fast queries**: `knots ls`, filtering, dependency views, etc., should be instant.
- **Supports graph features**:
  - Dependencies (“ties”): blocked-by / blocks
  - Hierarchy: parent/child (epics/strands)
- **Supports your workflow states**:
  - `idea`
  - `work_item`
  - `implementing`
  - `implemented`
  - `reviewing`
  - `rejected`
  - `refining`
  - `approved`
  - `shipped`
  - `deferred`
  - `abandoned`
- **Supports iteration routing**: track “rework cycles” without forcing extra states.

### Product Principles (non-negotiable)
1) **Blazing fast CRUD performance for normal operations**
   - Most commands must be **cache-first** and complete without network.
   - Reads must not traverse git history; they must hit indexed local state.

2) **Responsive remote syncs**
   - A typical sync (few new events, healthy network) should complete in **< 1 second** end-to-end.
   - If sync can’t complete quickly (slow network / lock contention), Knots should return cached results and **defer sync** rather than blocking UX.

3) **Invisible to the user**
   - Users never run `git commit/push/pull` for knots; the CLI owns it.
   - Knots must not require users to switch branches or resolve merge conflicts.
   - Minimal/no “status spam” — only surface issues when correctness would be impacted.

### Performance Targets (design budgets, not hard guarantees)
- **Read path (hot/warm lists, filters):** p50 < 20ms, p99 < 150ms (SQLite).
- **Write path (create/update):** p50 < 150ms, p99 < 800ms *excluding network*.
- **Auto-sync attempt:** budgeted (default 750ms). If it overruns, return cached output and mark “sync pending”.
- **Explicit `knots sync`:** < 1s under normal conditions (small deltas; healthy network).

### Constraints
- **No Dolt**
- **No giant mutable JSON** blobs in git.
- **No manual `commit/push/pull` for users**: the CLI does all git operations.

---

## 2. High-Level Design

**Core idea**:  
Git is the replication layer; Knots data is stored on a dedicated branch as an **append-only event log**. A local **SQLite cache** provides fast, indexed queries.

- **Source of truth**: append-only event files in git
- **Fast read model**: local SQLite materialized view
- **Writes**: create event files → commit → push (all done by CLI)
- **Reads**: load from SQLite; optionally auto-sync first
- **Same-host concurrency**: coordinated via file locks (repo lock + cache lock)

---

## 3. Git Layout: Dedicated Branch, No PR Noise

### Branches
- `knots` (default): stores all knots data (events, snapshots, etc.)

Code branches (e.g., `main`) never include knots data in their trees, so PR diffs remain clean.

### Worktree Strategy (recommended)
Knots uses a **hidden git worktree** checked out at branch `knots`.

- Worktree path (local, gitignored):
  - `.knots/_worktree/` (inside repo root), or
  - `${XDG_STATE_HOME}/knots/<repo-id>/worktree/` (outside repo)

Users keep working on their code branch; Knots writes only to its own worktree.

---

## 4. Repository Data Model (Event Sourcing)

### 4.1 Event Streams

We store two streams:

1) **Full events** (can be larger; includes comments, body, etc.)  
2) **Index events** (small “headline” deltas; optimized for syncing/listing)

This enables **hot/warm/cold** strategies without scanning everything.

#### Why two streams?
- **Index events** let clients quickly learn: id, title, state, updated_at (and little else).
- **Full events** are only needed when a knot is **hot** (recently updated) or explicitly **rehydrated**.

### 4.2 Event File Storage (append-only)

In the `knots` branch:

```
.knots/
  events/            # full event stream (potentially larger payloads)
    2026/02/22/<event_id>-knot.state_set.json
    2026/02/22/<event_id>-knot.comment_added.json
  index/             # small index-delta stream
    2026/02/22/<event_id>-idx.knot_head.json
  snapshots/         # optional optimization snapshots (append-only)
    <asof_ts>-active_catalog.msgpack.zst
    <asof_ts>-cold_catalog.msgpack.zst
  config/
    schema_version.txt
```

**Event files are never edited** after being committed; only new files are added.

### 4.3 Event Identifiers

Use a sortable unique ID:
- **UUIDv7** (preferred) or **ULID**

Requirements:
- Unique across hosts
- Encodes time ordering (helps deterministic replay order)
- Used in filename to avoid collisions

### 4.4 Minimal Event Types

#### Full stream (`.knots/events/...`)
- `knot.created` — id, title, initial state, body (optional)
- `knot.title_set`
- `knot.body_set` (or `knot.note_added`)
- `knot.state_set`
- `knot.comment_added`
- `knot.tag_add` / `knot.tag_remove`
- `knot.edge_add` / `knot.edge_remove` (deps + hierarchy)
- `knot.review_decision` (approved/rejected + categories)

#### Index stream (`.knots/index/...`)
- `idx.knot_head` — minimal delta describing *current* headline state:
  - `knot_id`
  - `title`
  - `state`
  - `updated_at`
  - (optional) `terminal` boolean (derived from state, but storing is convenient)

> **Rule**: every write that changes title/state/updated_at MUST also emit an `idx.knot_head` event.

### 4.5 Iteration / Rework (routing-friendly)

To capture “refining is just implementing” without relying on a special state:

Emit explicit review decisions:

- `knot.review_decision { outcome: "approved" | "rejected", categories: [...], notes: ... }`

Then compute:
- `rework_count` = number of rejected decisions
- `last_reject_categories`
- `last_decision_at`

This lets operators/agents route dynamically:
- `rework_count >= 2` → stronger agent / human check
- category includes `requirements` → route back to planning/refinement
- category includes `tests` → route to verification agent

---

### 4.6 Optimistic Preconditions (Workflow ETags / If-Match)
Knots is append-only in git, so file-level conflicts are rare. The harder problem is **semantic correctness**:
making sure a human/agent is not appending “next step” events based on stale state.

Knots supports **optimistic concurrency control (OCC)** using an opaque **workflow ETag** token:

- Each knot has a current `workflow_etag` (an opaque string), representing “the latest workflow-relevant index event we’ve applied”.
- A worker reads a knot and captures the `workflow_etag` **before** doing expensive work.
- When the worker is ready to append a new event, it re-checks the current `workflow_etag`.
  - If it matches the expected token → append and commit.
  - If it differs → **discard/retry** (the knot changed while we worked).

**Important:** the ETag is **not ordered**. It is not “highest ID” and not “latest timestamp”.
It is an opaque token used only for **equality** (“has the head changed?”), analogous to HTTP `If-Match`.

#### What advances the workflow ETag?
The workflow ETag advances only on **workflow-relevant index updates** (default set):
- state changes (`knot.state_set`)
- title changes (`knot.title_set`)
- dependency/hierarchy changes (`knot.edge_add/remove`)
- routing metadata changes (tags/assignee/labels) *if used by routing*

To avoid spurious invalidation, **pure activity** (e.g., comments) should not advance `workflow_etag`
unless your automation cares about it.

Practical implementation:
- Maintain `workflow_etag` in SQLite as “the last applied `idx.knot_head.event_id` for this knot”.
- If you later decide comments should make an item “hot”, emit a separate tiny `idx.knot_activity`
  event for activity timestamps without changing `workflow_etag`.

#### Precondition encoding (optional, extra safety)
For defense-in-depth, Knots may embed the expected token inside emitted events:

```json
{
  "type": "knot.state_set",
  "knot_id": "K-123",
  "precondition": { "workflow_etag": "AHx-14" },
  "data": { "to": "implemented" }
}
```

Reducers/indexers can treat events whose precondition fails as **stale** and ignore them.
(With the repo-lock + push-retry flow, these stale events should be extremely rare.)

#### Worker pattern (long-running)
1) Read knot → capture `workflow_etag`
2) Perform expensive work (LLM, tests, codegen) **without locks**
3) At write time, acquire `repo_lock`, fast-forward, and validate `workflow_etag` before appending

This keeps the critical section **very small**, improving same-host concurrency and perceived performance.

#### Human UX (still “invisible”)
- Normal short commands (`knots state K-1 implemented`) can omit ETags; the CLI reads the current token immediately before writing.
- Long-running human workflows (hours-long) can be protected by surfacing an optional `--if-match <etag>` flag.

## 5. Local Cache (SQLite) — Fast Reads, Tiering

Knots maintains a local SQLite DB (not in git):

- Location: `.knots/cache/state.sqlite` (gitignored)  
  or `${XDG_CACHE_HOME}/knots/<repo-id>/state.sqlite`

### 5.1 SQLite Concurrency Settings
- WAL mode (`PRAGMA journal_mode=WAL;`)
- Busy timeout (`PRAGMA busy_timeout=5000;`)
- Readers run concurrently; a single writer updates during sync/rehydration.

### 5.2 Suggested Tables (high-level)

**Concurrency token**
- `workflow_etag` (opaque string): stored per knot in the cache to support OCC/If-Match.
  - Implementation: set to the latest applied `idx.knot_head.event_id` for that knot.
  - Note: warm UI can still show only `id+title`; the cache may keep internal tokens/state to support eviction and correctness.

**Meta**
- `meta(key TEXT PRIMARY KEY, value TEXT)`  
  - `last_index_head_commit`
  - `last_full_head_commit`
  - `schema_version`
  - `hot_window_days` (default 7)

**Hot knots (fully hydrated)**
- `knot_hot(id TEXT PRIMARY KEY, title TEXT, state TEXT, updated_at TEXT, body TEXT, ... )`

**Warm knots (headlines only)**
- `knot_warm(id TEXT PRIMARY KEY, title TEXT)`  
  *(strictly id+title per requirement)*

**Edges (only for hydrated knots unless you choose otherwise)**
- `edge(src TEXT, kind TEXT, dst TEXT, PRIMARY KEY (src, kind, dst))`
  - `kind ∈ {"blocks", "blocked_by", "parent_of"}`

**Review stats**
- `review_stats(id TEXT PRIMARY KEY, rework_count INT, last_decision_at TEXT, last_outcome TEXT, ...)`

**Cold catalog (loaded on-demand only)**
- `cold_catalog(id TEXT PRIMARY KEY, title TEXT, state TEXT, updated_at TEXT)`  
  - Not populated unless the user runs `knots cold sync`.

---

## 6. Hot / Warm / Cold Storage Strategy

### 6.1 Definitions

Let:
- `HOT_WINDOW_DAYS = N` (default `7`)
- Terminal states (cold): `{ shipped, deferred, abandoned }`

Classification:
1. **Cold** if state ∈ terminal states  
   - **Not synced** by default (no id/title stored in warm/hot tables)
   - Requires manual `cold sync` + `rehydrate`
2. **Hot** if not terminal AND `updated_at >= now - N days`  
   - Fully hydrated (full metadata imported into `knot_hot`)
3. **Warm** otherwise (not terminal, older than window)  
   - Only `id + title` kept in `knot_warm`  
   - Metadata loaded only on-demand (“rehydrate”)

> Cold overrides hot, even if recently updated.

### 6.2 Default Sync Behavior
`knots sync` does:
- Fetch and fast-forward `knots` branch
- Apply **index events** to build/update warm/hot sets
- Hydrate **hot** knots by replaying full events (for those knots only)
- Enforce eviction/demotion rules

### 6.3 Cold Sync (manual)
`knots cold sync` does:
- Fetch/fast-forward (same as sync)
- Populate/refresh `cold_catalog` table using:
  - `cold_catalog` snapshot if present, else
  - scanning index deltas for terminal states (slower)
- Does **not** hydrate full metadata

Then user can:
- `knots cold search <term>`
- select `id/title`
- `knots rehydrate <id>`

### 6.4 Rehydration

**Warm rehydrate**:  
When user runs `knots show K-123` and it’s warm:
- Load full events for K-123 and build full state
- Move into `knot_hot` (locally cached)
- Optional: keep it hot for `access_ttl` even if old (configurable)

**Cold rehydrate**:
- Requires `cold sync` to discover candidates
- Then hydrate same as above

---

### 6.5 Sync Policy (auto / always / never)
Knots supports a sync policy to satisfy “fast + invisible”:

- `auto` (default): **cache-first**, then attempts a **budgeted** remote sync (default 750ms).
  - If the repo lock is busy (another client writing), auto-sync is skipped.
  - If the fetch/apply overruns the time budget, Knots returns cached results and marks **sync pending**.
- `always`: performs a full sync before serving results (may block; best for CI).
- `never`: never touches the network (offline mode); uses local cache only.

Config keys (suggested):
- `sync.policy = "auto" | "always" | "never"`
- `sync.auto_budget_ms = 750`
- `sync.try_lock_ms = 0` (don’t block reads)
- `sync.fetch_args = ["--no-tags", "--prune"]`

## 7. Sync and Incremental Processing

### 7.1 Sync uses git-diff to find new files
To keep syncs responsive (<1s under normal conditions), the implementation should:
- Fetch **only the knots branch** and avoid tags: `git fetch --no-tags --prune origin knots`.
- Prefer **small, index-only blobs** by keeping `.knots/index` events tiny (≈1–2KB).
- Optionally use **partial clone filters** to avoid downloading large “full event” blobs during normal sync:
  - Example: `git fetch --filter=blob:limit=4k ...` so index blobs arrive, large bodies stay lazy until rehydrate.
- Avoid scanning the working tree; derive deltas via `git diff --name-status <old>..<new>`.
- Apply index events incrementally and hydrate only the hot set (last N days).

Because events are append-only, the fastest way to process updates is:

- Track `last_index_head_commit` in SQLite
- On each sync:
  - determine new head commit on `origin/knots`
  - `git diff --name-status <old>..<new> -- .knots/index/` to find new index event files
  - parse and apply only those

Similarly for full events:
- `git diff --name-status <old_full>..<new> -- .knots/events/` then only hydrate the knots we care about (hot list)

### 7.2 Snapshot Optimizations (optional)
To reduce “cold sync” cost and bootstrap time:
- Store append-only snapshots:
  - `snapshots/<ts>-active_catalog.msgpack.zst`
  - `snapshots/<ts>-cold_catalog.msgpack.zst`

Snapshots are **derived** and can be generated by any client:
- `knots compact --write-snapshots`
- CI can generate nightly

Clients choose the latest snapshot by filename timestamp.

---

## 8. Locking Model (Same-Host Robustness)

We use two advisory locks:

1) **Repo lock** (exclusive): serializes git mutations  
   - protects: `fetch`, `checkout/worktree`, `commit`, `push`, ref updates, index writes  
   - file: `${repo}/.git/knots.lock` (or XDG state dir)

2) **Cache lock** (exclusive for writers): serializes SQLite writes  
   - file: `${cache_dir}/cache.lock`

### 8.1 Lock Ordering (avoid deadlocks)
If an operation needs both:
- Always acquire `repo_lock` **before** `cache_lock`.

### 8.2 What takes which lock?

| Operation | Repo Lock | Cache Lock |
|---|---:|---:|
| `knots ls` (no sync) | No | No |
| `knots ls --sync` | Yes | Yes |
| `knots sync` | Yes | Yes |
| Any write (`new`, `state`, `tie add`, etc.) | Yes | (Yes, to update cache) |
| `rehydrate` | Yes (if it needs fetch) | Yes |
| `cold sync` | Yes | Yes |


### 8.3 “Invisible” Concurrency: Try-Lock for Auto-Sync Reads
To keep the UX responsive when multiple clients run on the same host:

- **Read commands** in `sync.policy=auto` should attempt to acquire `repo_lock` with **try-lock** semantics (0ms or small budget).
- If the lock is held (another client is writing/syncing), the read command should:
  - return cached results immediately
  - optionally print nothing (default) and just mark `sync_pending=true` in cache meta

This prevents a TUI from “hitching” when another terminal is doing a write/push.


**Why this matters:** OCC keeps critical sections small. A worker can spend minutes doing LLM/code/test work
and only take `repo_lock` for a short, deterministic “validate head → append” window.
---

## 9. Git Write Flow: No Manual Commit/Push/Pull

### 9.1 “Invisible” Networking: Local-First Writes + Opportunistic Push
To satisfy “blazing fast CRUD” and “invisible UX”, **writes are local-first**:
- The CLI always records the change locally (new event files + local commit).
- It then attempts to push within a small time budget.
- If pushing can’t complete quickly (offline, remote contention), the commit remains local and will be pushed later during a future `knots sync` or write.

Optional UX:
- `knots status` can show whether the local knots branch is ahead of `origin/knots` (unsent commits).
- Normal commands remain silent unless the user explicitly asks.

A mutating command does:

1. Acquire `repo_lock`
2. Ensure worktree exists and is clean
3. Fetch `origin/knots`
4. Fast-forward local `knots` worktree to `origin/knots`
5. Create event files (atomic writes)
6. `git add`, `git commit`
7. `git push` with retry
8. Release `repo_lock`
9. Acquire `cache_lock`
10. Update SQLite (apply new index/full events)
11. Release `cache_lock`

---

## 10. Exact Locking + Retry Pseudocode (Rust-like)

### 10.1 Utilities

```rust
// Pseudocode types
type Result<T> = std::result::Result<T, KnotsError>;

struct Repo {
    root: PathBuf,          // code repo root
    git_dir: PathBuf,       // root/.git
    knots_branch: String,   // "knots"
    remote: String,         // "origin"
    worktree: PathBuf,      // root/.knots/_worktree (gitignored)
    cache_db: PathBuf,      // root/.knots/cache/state.sqlite (gitignored)
    locks_dir: PathBuf,     // maybe root/.git or XDG state dir
}

// Advisory file lock using fs4 or fs2
fn lock_exclusive(path: &Path, timeout_ms: u64) -> Result<LockGuard> {
    // open/create lock file
    // loop with try_lock + sleep until timeout
    // return guard that unlocks on drop
}

// Try-lock (non-blocking) for “invisible” auto-sync reads
fn try_lock_exclusive(path: &Path) -> Result<Option<LockGuard>> {
    // open/create lock file
    // attempt a single try_lock
    // return Some(guard) if acquired, None otherwise
}

fn now_utc_iso() -> String { /* ... */ }
fn uuidv7() -> String { /* ... */ }
```

### 10.2 Acquire Repo Lock Wrapper

```rust
fn with_repo_lock<T>(repo: &Repo, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let lock_path = repo.locks_dir.join("knots.lock");
    let _guard = lock_exclusive(&lock_path, /*timeout*/ 30_000)?;
    f()
}

// Non-blocking variant used by auto-sync reads.
// If the lock is busy, we skip sync and serve cached results.
fn try_with_repo_lock<T>(repo: &Repo, f: impl FnOnce() -> Result<T>) -> Result<Option<T>> {
    let lock_path = repo.locks_dir.join("knots.lock");
    match try_lock_exclusive(&lock_path)? {
        Some(_guard) => Ok(Some(f()?)),
        None => Ok(None),
    }
}

fn with_cache_lock<T>(repo: &Repo, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let lock_path = repo.cache_db.parent().unwrap().join("cache.lock");
    let _guard = lock_exclusive(&lock_path, /*timeout*/ 30_000)?;
    f()
}
```

### 10.3 Fetch + Fast-Forward (no rebase required)

Because our commits only **add new unique files**, we can avoid complicated rebases.
On push rejection, we reset to remote head and re-apply the same event files.

```rust
fn git_fetch(repo: &Repo) -> Result<()> { /* run: git fetch <remote> <branch> */ }
fn git_rev_parse(repo: &Repo, rev: &str) -> Result<String> { /* git rev-parse */ }
fn git_reset_hard(repo: &Repo, rev: &str) -> Result<()> { /* git reset --hard <rev> */ }
fn git_status_clean(repo: &Repo) -> Result<bool> { /* ensure no staged/unstaged changes */ }

fn ff_to_remote(repo: &Repo) -> Result<String> {
    git_fetch(repo)?;
    let remote_head = git_rev_parse(repo, &format!("{}/{}", repo.remote, repo.knots_branch))?;
    git_reset_hard(repo, &remote_head)?;
    Ok(remote_head)
}
```

### 10.4 Atomic Event File Write

```rust
struct EventFile {
    rel_path: PathBuf,  // e.g. ".knots/index/2026/02/22/<id>-idx.knot_head.json"
    bytes: Vec<u8>,
}

fn write_event_files_atomically(repo: &Repo, files: &[EventFile]) -> Result<()> {
    for f in files {
        let abs = repo.worktree.join(&f.rel_path);
        let tmp = abs.with_extension("tmp");

        // ensure parent dirs
        create_dir_all(abs.parent().unwrap())?;

        // write tmp
        write(&tmp, &f.bytes)?;
        fsync_file(&tmp)?;
        // atomic rename
        rename(tmp, abs)?;
    }
    Ok(())
}
```

### 10.5 Commit

```rust
fn git_add(repo: &Repo, paths: &[PathBuf]) -> Result<()> { /* git add -- <paths> */ }
fn git_commit(repo: &Repo, message: &str) -> Result<String> { /* git commit -m */ }
```

### 10.6 Push Retry Loop (handles concurrent writers)

```rust
enum PushErr {
    NonFastForward,
    Transient(String),
    Fatal(String),
}

enum PushOutcome {
    // remote now includes our commit
    Pushed { commit: String },
    // we committed locally but couldn't push within the budget;
    // remote will catch up on a later sync/write.
    Queued { commit: String, reason: String },
}

fn git_push(repo: &Repo) -> Result<std::result::Result<(), PushErr>> { /* ... */ }

fn push_with_retry_budget(
    repo: &Repo,
    event_files: &[EventFile],
    commit_msg: &str,
    total_budget_ms: u64,
) -> Result<PushOutcome> {
    const MAX_ATTEMPTS: u32 = 8;

    let start = monotonic_ms();

    for attempt in 0..MAX_ATTEMPTS {
        // If we’re out of time, stop trying — keep it invisible and fast.
        if monotonic_ms().saturating_sub(start) > total_budget_ms {
            // Ensure we still have a local commit that includes the event files.
            // If no commit exists yet, do one last local-only commit (no push).
            let remote_head = ff_to_remote(repo)?;
            if !git_status_clean(repo)? { return Err(KnotsError::DirtyWorktree); }
            write_event_files_atomically(repo, event_files)?;
            let add_paths: Vec<PathBuf> = event_files.iter().map(|e| e.rel_path.clone()).collect();
            git_add(repo, &add_paths)?;
            let local_commit = git_commit(repo, commit_msg)?;
            return Ok(PushOutcome::Queued {
                commit: local_commit,
                reason: format!("push budget exceeded; remote_head={}", remote_head),
            });
        }

        // Always start from remote head to minimize rejection
        let _remote_head = ff_to_remote(repo)?;

        if !git_status_clean(repo)? {
            return Err(KnotsError::DirtyWorktree);
        }

        // write files and commit
        write_event_files_atomically(repo, event_files)?;
        let add_paths: Vec<PathBuf> = event_files.iter().map(|e| e.rel_path.clone()).collect();
        git_add(repo, &add_paths)?;
        let local_commit = git_commit(repo, commit_msg)?;

        // try push
        match git_push(repo)? {
            Ok(()) => return Ok(PushOutcome::Pushed { commit: local_commit }),
            Err(PushErr::NonFastForward) => {
                // Another client pushed; retry.
            }
            Err(PushErr::Transient(_)) => {
                // exponential backoff with jitter (but respect total_budget_ms)
                let delay = backoff_ms(attempt);
                sleep_ms(delay);
            }
            Err(PushErr::Fatal(msg)) => {
                // Keep local commit (data safe), but surface a real error.
                return Err(KnotsError::GitPushFailed(msg));
            }
        }
    }

    // Give up pushing, but keep local commit and return queued.
    Ok(PushOutcome::Queued {
        commit: git_rev_parse(repo, "HEAD")?,
        reason: "push retries exhausted".to_string(),
    })
}
```

**Why this is robust**
- Non-fast-forward is handled automatically.
- No interactive conflict resolution is required because the commit only adds unique files.
- If a collision happens, it means event file naming is broken (IDs not unique).

### 10.7 Full Write Command Example: `knots state <id> <new_state>`

```rust
fn cmd_state_set(repo: &Repo, knot_id: &str, new_state: &str) -> Result<()> {
    // Build events in memory first (fast, no locks needed yet)
    let ts = now_utc_iso();
    let eid1 = uuidv7();
    let eid2 = uuidv7();

    let full = EventFile {
        rel_path: path_for_full_event(&ts, &eid1, "knot.state_set"),
        bytes: build_json_full_state_set(eid1, ts.clone(), knot_id, new_state),
    };

    let idx = EventFile {
        rel_path: path_for_index_event(&ts, &eid2, "idx.knot_head"),
        bytes: build_json_idx_head(eid2, ts.clone(), knot_id, /*title*/ None, /*state*/ Some(new_state)),
    };

    let files = vec![full, idx];

    // Repo mutation is serialized; keep push invisible/fast by budgeting it.
    let _outcome = with_repo_lock(repo, || {
        ensure_knots_worktree(repo)?;
        push_with_retry_budget(
            repo,
            &files,
            &format!("knots: {} state -> {}", knot_id, new_state),
            /*total_budget_ms*/ 800,
        )
    })?;

    // Update cache up to our local HEAD (whether or not remote push succeeded).
    with_cache_lock(repo, || {
        let head = git_rev_parse(repo, "HEAD")?;
        cache_apply_up_to(repo, &head)?;
        Ok(())
    })?;

    // Default: no user-visible output about queued pushes.
    // Expose it via `knots status` if desired.
    Ok(())
}
```

### 10.7a If-Match Variant (workflow ETag precondition)

```rust
fn cmd_state_set_if_match(
    repo: &Repo,
    knot_id: &str,
    new_state: &str,
    expected_workflow_etag: &str,
) -> Result<()> {
    // Fast path: build events without locks
    let ts = now_utc_iso();
    let eid1 = uuidv7();
    let eid2 = uuidv7();

    // Repo mutation is serialized; validate precondition under repo_lock.
    with_repo_lock(repo, || {
        ensure_knots_worktree(repo)?;
        let remote_head = ff_to_remote(repo)?;

        // Bring cache up-to-date enough to read current workflow_etag.
        with_cache_lock(repo, || {
            cache_apply_up_to(repo, &remote_head)?;
            Ok(())
        })?;

        let current = with_cache_lock(repo, || cache_get_workflow_etag(repo, knot_id))?
            .unwrap_or_default();

        if current != expected_workflow_etag {
            return Err(KnotsError::StaleWorkflowHead {
                expected: expected_workflow_etag.to_string(),
                current,
            });
        }

        // Build event payloads with embedded precondition (optional)
        let full = EventFile {
            rel_path: path_for_full_event(&ts, &eid1, "knot.state_set"),
            bytes: build_json_full_state_set_with_precond(
                eid1, ts.clone(), knot_id, new_state, expected_workflow_etag,
            ),
        };

        let idx = EventFile {
            rel_path: path_for_index_event(&ts, &eid2, "idx.knot_head"),
            bytes: build_json_idx_head_with_precond(
                eid2, ts.clone(), knot_id, /*title*/ None, /*state*/ Some(new_state),
                expected_workflow_etag,
            ),
        };

        let files = vec![full, idx];

        // Budgeted push keeps UX fast; if it can’t push, it queues locally.
        let _outcome = push_with_retry_budget(
            repo,
            &files,
            &format!("knots: {} state -> {}", knot_id, new_state),
            /*total_budget_ms*/ 800,
        )?;

        Ok(())
    })?;

    // Update cache to our local HEAD (whether or not remote push succeeded)
    with_cache_lock(repo, || {
        let head = git_rev_parse(repo, "HEAD")?;
        cache_apply_up_to(repo, &head)?;
        Ok(())
    })?;

    Ok(())
}
```

### 10.8 Sync Command: `knots sync`

```rust
fn cmd_sync(repo: &Repo) -> Result<()> {
    let target_head = with_repo_lock(repo, || {
        ensure_knots_worktree(repo)?;
        ff_to_remote(repo) // returns remote head commit we reset to
    })?;

    with_cache_lock(repo, || {
        cache_apply_up_to(repo, &target_head)?;
        cache_demote_and_evict(repo)?;
        Ok(())
    })?;

    Ok(())
}
```

### 10.9 Auto-Sync Helper (cache-first, try-lock, time budget)

```rust
fn maybe_auto_sync(repo: &Repo) -> Result<()> {
    // 1) Serve reads from cache regardless.
    // 2) Try to sync quickly if lock is available and time budget allows.

    if repo.sync_policy != "auto" { return Ok(()); }

    // Non-blocking repo lock: if another client is writing, skip.
    let did_sync = try_with_repo_lock(repo, || {
        ensure_knots_worktree(repo)?;
        // Budgeted sync: fetch+apply should not exceed auto_budget_ms.
        let head = ff_to_remote(repo)?;
        with_cache_lock(repo, || {
            cache_apply_up_to(repo, &head)?;
            cache_demote_and_evict(repo)?;
            Ok(())
        })?;
        Ok(())
    })?;

    if did_sync.is_none() {
        // Record sync_pending=true in SQLite meta (optional) so UI can show a subtle indicator.
        with_cache_lock(repo, || { cache_set_sync_pending(repo, true) })?;
    }

    Ok(())
}
```

---

## 11. Cache Update Details (Hot/Warm/Cold)

### 11.1 Apply Index Deltas
Process new `.knots/index/...` files since `last_index_head_commit`:

For each `idx.knot_head` delta:
- Determine terminal: `state ∈ {shipped, deferred, abandoned}`
- If terminal:
  - remove from `knot_hot` and `knot_warm`
  - (optionally) insert into `cold_catalog` only if cold sync mode is enabled
- Else:
  - compute `is_hot` from `updated_at`
  - if hot:
    - ensure in “hot set” for hydration
  - else:
    - ensure `knot_warm(id,title)` exists and remove from `knot_hot` if present

**Note**: `knot_warm` stores only `id+title` per requirement.

### 11.2 Hydrate Hot Knots (Full Events)
For knots classified as hot:
- find all new full event files for those knot IDs since `last_full_head_commit`
- replay them into a “current state” reducer
- write full state into `knot_hot` (replace row)
- update edges/tags/review_stats as needed

### 11.3 Demote/Evict Sweep
On each sync:
- Demote any hot knots whose `updated_at` is now older than window:
  - delete from `knot_hot`
  - ensure in `knot_warm` (id,title)
- Evict any knots that became terminal (cold):
  - delete from `knot_hot` and `knot_warm`

---

## 12. Warm/Cold Discovery UX

### Default
- `knots ls` shows hot + warm (warm with only id/title; hot with full data)

### Cold
- `knots cold sync` (manual) loads cold catalog (id/title/state/updated_at)
- `knots cold search "foo"` lists matching cold knots
- `knots rehydrate K-999` loads full metadata and places it into local hot cache

> You can optionally auto-evict rehydrated cold knots after an access TTL.

---

## 13. Integrity, Validation, and “fsck”

Implement:
- `knots fsck`:
  - validate event JSON schema
  - validate required fields
  - validate edge references
  - detect duplicate event IDs / filename collisions
- `knots doctor`:
  - checks locks, worktree health, remote reachability
  - detects known conflicts (e.g., if worktree is dirty)

---

## 14. Rust Implementation Notes

Suggested crates:
- CLI: `clap`
- Errors: `anyhow` or `thiserror`
- JSON: `serde`, `serde_json`
- IDs: `uuid` (v7), or `ulid`
- Time: `time` or `chrono`
- SQLite: `rusqlite`
- Compression snapshots: `zstd`, `rmp-serde` (MsgPack)
- Locks: `fs4` (cross-platform file locking)

Git integration:
- **Recommended**: call `git` via `std::process::Command` for:
  - worktrees
  - fetch/reset/commit/push
  - diff name-status
- Alternative: `git2` (libgit2) for finer control, but more complexity and gaps.

---

## 15. Future: Stronger Cold Storage (Network/Size Optimization)

If the knots branch grows huge, consider:
- **Partial clone** with blob filters (`--filter=blob:none`) so cold blobs aren’t downloaded until needed.
- Store large comment bodies as compressed blobs (still in git), or split into `.zst` payload files.

These are optional optimizations; the hot/warm/cold cache tiering already yields most performance wins.

---

## 16. Summary

- **Dedicated `knots` branch** → no PR noise
- **Append-only event files** → merge-friendly, robust
- **Index stream + SQLite cache** → fast lists + scalable warm/cold strategy
- **Repo lock + cache lock** → safe multi-client host behavior
- **Push retry loop** → no manual git required
- **Hot/warm/cold**:
  - hot: fully hydrated (updated in last N days)
  - warm: id+title only
  - cold: invisible until manual cold sync + rehydrate

---

## Appendix A: State Machine (pragmatic)

Suggested “common path” transitions (validation/UX only):

- `idea -> work_item`
- `work_item -> implementing`
- `implementing -> implemented`
- `implemented -> reviewing`
- `reviewing -> approved` OR `reviewing -> rejected`
- `rejected -> refining -> implemented` (loop)
- `approved -> shipped`
- `* -> deferred` (pause)
- `* -> abandoned` (terminal)

Validation should be *helpful, not restrictive*: allow `--force` to bypass if needed.

