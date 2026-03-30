# sync

Git-based event replication between local and remote.

## Key Files

- **`mod.rs`** — `pull()`, `push()`, `sync()` entry points
- **`apply.rs`** — `IncrementalApplier`: applies index and full events to SQLite cache
- **`apply_helpers.rs`** — helper functions for event application
- **`git.rs`** — git operations (fetch, reset, commit, push)
- **`worktree.rs`** — `KnotsWorktree`: manages the `.knots/_worktree` git worktree

## Data Flow

```
push: scan local events -> copy to worktree -> git commit -> git push
pull: git fetch -> reset worktree -> apply index events -> apply full events -> update cache
```
