# Portfolio Plan

## Summary

Introduce a repo-owned `portfolio` concept that groups related Knots projects
for discovery and cross-project visibility. A portfolio is defined from within
an owning Knots project. In phase 1, portfolios are a read/query feature, not
an execution scope.

Phase 1 delivers:

- Synced portfolio definitions owned by one Knots project/repo
- Portfolio member records for both git-backed repos and named local projects
- Display-only cross-project dependency links
- `kno portfolio ...` commands, including JSON output
- `kno ls --external-links` to decorate local results and include linked
  external knots
- A verified local cache of known projects for selector UX and doctor cleanup

Phase 1 does not deliver:

- Portfolio-scoped `poll`, `ready`, `claim`, or workflow execution
- Cross-project `parent_of`
- Workflow-enforced cross-project blocking semantics
- Sharing portfolios owned by local-only projects

## Decisions Captured

- A project can be either a git-backed repo Knots store or an existing named
  local Knots project.
- A portfolio is owned by the project/repo where it is created.
- Only the owner project exposes and syncs its portfolio definitions.
- Local-only owner projects may define portfolios, but those portfolios remain
  local-only in phase 1.
- A project may belong to multiple portfolios.
- There is no active portfolio mode.
- Cross-project links are visibility-only, with warnings or decorations as
  needed.
- Allowed cross-project edge kinds in v1: `blocked_by`, `blocks`, `relates_to`.
- Cross-project `parent_of` is explicitly out of scope.
- `kno edge add loom-12ef blocks foolery-f883` should work when the current
  project owns a portfolio that can resolve both project identifiers.
- If writing a duplicated external link cannot succeed on both sides, fail the
  whole command.
- `kno ls --external-links` should show raw knot ids for external references
  and should include the external knots themselves.
- Portfolio/project selectors should use a verified local cache of known
  projects and should be cleaned by `kno doctor --fix`.

## Design Goals

1. Keep existing single-project workflows stable.
2. Add a durable data model for portfolio membership and external links.
3. Keep external link semantics simple and legible in the CLI.
4. Make project resolution predictable when repo basenames collide.
5. Allow a clean later path to portfolio-aware execution without forcing it now.

## Proposed Model

### Portfolio Ownership

Each portfolio has one owning project.

- Creating a portfolio inside a git-backed Knots repo makes that repo the owner.
- Creating a portfolio inside a named local project makes that named project the
  owner, but the portfolio is local-only in phase 1.
- The owner project is the only store that persists the membership definition.

This keeps sync simple: portfolio definitions travel with the owner project's
`knots` branch instead of introducing a new global sync plane.

### Portfolio Identity

Use a simple user-facing portfolio name, unique within the owning project.

Suggested key:

- `owner_project_key + portfolio_name`

Suggested user-facing syntax:

- `main/backend`
- `feature-builder/main`

The name only needs to be unique inside the owner project, because ownership is
part of the effective identity.

### Project Reference

Portfolio membership should persist a logical project reference, not a path.

Suggested shape:

```json
{
  "key": "foolery",
  "aliases": ["foo"],
  "kind": "git",
  "remote_locator": {
    "git_remote": "git@github.com:acartine/foolery.git",
    "email": "team-foolery@example.com"
  },
  "named_project_id": null,
  "repo_basename": "foolery"
}
```

For named local projects:

```json
{
  "key": "demo-local",
  "aliases": ["demo"],
  "kind": "named_project",
  "remote_locator": {
    "email": "owner@example.com"
  },
  "named_project_id": "demo-local",
  "repo_basename": null
}
```

Rules:

- `key` is the stable portfolio member identifier.
- `aliases` exist to resolve collisions like `acartine/foo` and `bcartine/foo`.
- `repo_path` is never the persisted identity. It belongs in the local cache.
- For git repos, prefer the origin URL as the best available remote locator.
- Include an optional email/contact field when available so a missing member is
  still actionable.

### Local Known-Project Cache

Knots should maintain a local cache of known projects discovered over time from:

- `kno project ...` activity
- Knots repo commands run from git-backed repos
- portfolio member selection flows

Suggested local cache fields:

- canonical repo path
- named project id if known
- derived project key candidates
- latest verified timestamp
- whether `.knots` exists and is initialized

Verification behavior:

- selector commands verify cached paths before presenting them
- invalid entries are skipped with a warning
- `kno doctor` reports stale/missing entries
- `kno doctor --fix` removes invalid cache entries

## Event and Index Model

Phase 1 should represent portfolios and external links as new event types.

### New Full Event Types

- `portfolio.created`
- `portfolio.deleted`
- `portfolio.member_added`
- `portfolio.member_removed`
- `portfolio.member_alias_set`
- `knot.external_edge_add`
- `knot.external_edge_remove`

### New Index Event Types

- `idx.portfolio_head`
- `idx.external_edge`

Rationale:

- Portfolio definitions need their own projection path; overloading
  `idx.knot_head` would make the cache and sync logic harder to reason about.
- External links are not normal local edges and should not be fed into the
  existing state/hierarchy logic.

### Cache Tables

Add new cache projections instead of stretching `knot_hot` and `edges`.

Suggested tables:

- `portfolio_head`
- `portfolio_members`
- `external_edges`
- `known_projects` for the local verified cache

Suggested `external_edges` columns:

- `local_knot_id`
- `local_project_key`
- `kind`
- `remote_project_key`
- `remote_knot_id`
- `remote_locator_json`
- `source_portfolio_name`
- `reciprocal` boolean
- `updated_at`

The `edges` table remains reserved for same-store semantic edges.

## Cross-Project Link Semantics

### Visibility Only

External links are query/UI data only in phase 1.

- They do not block state transitions.
- They do not affect `poll`, `ready`, `claim`, or auto-resolve logic.
- They do not participate in hierarchy rendering.

This keeps phase 1 from destabilizing workflow correctness.

### Duplication Strategy

Write a reciprocal external link record into both participating stores.

Recommended mapping:

- user command in project A:
  - `A blocks B`
- persisted in project A:
  - `A blocks B`
- persisted in project B:
  - `B blocked_by A`

For `relates_to`, persist the same symmetric relation in both stores.

Why this is the best default:

- Each project can render outbound and inbound external references locally.
- CLI queries do not require remote writes to be re-derived later.
- Semantics stay legible in each store.

Failure rule:

- If either side cannot be resolved or written, fail the whole command.

### Resolution Rules

Extend `kno edge add` to resolve raw ids in this order:

1. try normal local knot resolution
2. if unresolved, consult portfolios owned by the current project
3. resolve member key or alias from the raw knot id prefix
4. if multiple members match, raise an ambiguity error and show candidates

Example:

- `loom-12ef blocks foolery-f883`
- local project owns a portfolio containing member key `foolery`
- Knots resolves `foolery-f883` as an external knot in member `foolery`

This avoids inventing a second command surface unless later experience proves
that local and external edge creation need to diverge.

## CLI Surface

### Portfolio Commands

Add a new command family:

```bash
kno portfolio create <name>
kno portfolio list
kno portfolio list --json
kno portfolio show <name>
kno portfolio show <name> --json
kno portfolio add <name> --path /path/to/repo
kno portfolio add <name> --project <named-project-id>
kno portfolio add <name> --select
kno portfolio remove <name> <project-key>
kno portfolio alias <name> <project-key> <alias>
```

Behavior notes:

- `create` inside a repo or active named project implicitly binds ownership to
  that project.
- `add --path` only succeeds for a known local Knots repo path.
- `add --project` supports named local projects.
- `add --select` uses the verified local known-project cache and filters to
  actually initialized Knots projects.
- `list --json` is the phase 1 `getPortfolios` equivalent.

### `kno ls --external-links`

Extend `kno ls` with:

```bash
kno ls --external-links
kno ls --external-links --json
```

Text behavior:

- render normal local knot rows
- decorate rows that have external links with raw ids only, for example:
  `blocked_by foolery-f883`
- add linked external knots as additional rows, visually marked as external
  using a distinct color/style

JSON behavior:

- include `external_links` on each local knot
- include `external_knots` for linked remote knot heads that are available from
  local cache/state
- include a `missing` status when a referenced project exists in the portfolio
  definition but is not currently available locally

### `kno show`

No portfolio-scoped `show` changes are required in phase 1.

## Missing Member Handling

When a synced portfolio member cannot be resolved locally:

- keep the portfolio visible
- mark the member as `missing`
- surface remote locator data, especially git remote or email when present

Suggested `kno portfolio show` rendering:

- `foolery  missing  git@github.com:acartine/foolery.git`

This gives the user something actionable without pretending the project is
available.

## Sync and Apply Changes

### Replication

Extend sync/apply to understand:

- portfolio full events and `idx.portfolio_head`
- external edge full events and `idx.external_edge`

The owner repo remains the source of truth for portfolio membership. Other repos
only receive portfolio definitions if they sync that owner repo directly.

### Local-Only Owner Projects

In phase 1:

- allow creation of local-only portfolios
- persist them locally in that named project's store
- reject any attempt to advertise them as shared/synced

This preserves the ownership model while avoiding premature distributed
semantics for local-only projects.

## Implementation Plan

### Slice 1: Domain, Events, and Storage

- add domain types for portfolio records, member refs, known project cache
- add event/index enums and serialization tests
- add SQLite tables and migrations for `portfolio_head`, `portfolio_members`,
  `external_edges`, and `known_projects`
- extend fsck/sync/apply to validate and project new event types

### Slice 2: Known Project Cache and Selectors

- capture known-project observations from repo and project command flows
- add verification helpers for repo path and Knots initialization
- add doctor checks and `--fix` cleanup
- add selector plumbing reused by portfolio member add flows

### Slice 3: Portfolio CRUD

- implement app/project-layer portfolio create/list/show/add/remove/alias
- add CLI command parsing and text/JSON output
- support owner-project binding from current repo or active named project
- render missing members with locator hints

### Slice 4: External Edge Persistence

- extend `edge add/remove` to resolve external ids through owned portfolios
- persist reciprocal external link records in both stores
- fail atomically if either side cannot be completed
- keep local same-store edges on the existing semantic path

### Slice 5: Listing and UX

- extend list/query layer with external link joins
- add `kno ls --external-links`
- include linked external knots in text and JSON output
- visually differentiate external rows without changing raw ids

### Slice 6: Documentation and Hardening

- update README and command help
- add end-to-end tests for sync, missing members, alias disambiguation, and
  local cache verification
- run `make lint` and `make sanity`

## Risks

### Identity Ambiguity

Repo basenames will collide. Member key plus alias support is mandatory, not an
optional polish item.

### Partial Distributed State

Because phase 1 keeps external links visibility-only, the system can tolerate
missing member repos without corrupting workflow behavior.

### Write Coordination Across Stores

Reciprocal external edge writes touch two stores. The implementation must define
lock ordering and rollback behavior carefully to avoid partial success.

Recommended rule:

- acquire store locks in a deterministic order based on absolute store path
- stage both writes
- if either write fails, roll back both cache mutations and do not emit either
  final index event

### Scope Creep Into Execution Semantics

Do not let `external_edges` leak into `state_resolve`, `poll_claim`, or
`list_layout`. That would silently turn a display feature into a workflow
feature.

## Testing Strategy

- unit tests for portfolio and project-ref normalization
- migration tests for new tables
- sync/apply tests for new event types
- CLI tests for portfolio CRUD and JSON output
- CLI tests for selector verification and doctor cleanup
- integration tests for:
  - owner repo defines a portfolio with two members
  - one member is missing locally after sync
  - `kno edge add` resolves a remote raw id through the portfolio
  - reciprocal external edge writes succeed
  - write failure on one side aborts the whole operation
  - `kno ls --external-links` decorates local knots and includes remote knots

## Suggested Knot Breakdown

Parent:

- `Introduce portfolios and external links in Knots`

Children:

- `Add portfolio domain model, events, and cache tables`
- `Build verified known-project cache and selector flows`
- `Implement portfolio CLI and owner-scoped membership management`
- `Support reciprocal visibility-only external edge persistence`
- `Extend ls with external link decoration and remote knot rows`
- `Document portfolio behavior and harden with sync/doctor tests`

## Open Follow-Ups

- whether local-only portfolios should later gain a user-level export/import
  path
- whether `kno show` should eventually resolve external ids directly
- whether phase 2 should add portfolio-aware `ready` or `poll`
- whether phase 2 should expose a machine API beyond `portfolio list --json`
