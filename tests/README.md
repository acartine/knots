# tests

Integration tests exercising the CLI and full application stack.

## Key Files

- **`cli_main_paths.rs`** — core create/update/state/list workflows
- **`cli_dispatch.rs`** — write operation dispatch and output formatting
- **`cli_dispatch_agent.rs`** — agent-specific dispatch (poll, claim, next)
- **`cli_dispatch_gate.rs`** — gate evaluation and reopen flows
- **`cli_dispatch_metadata.rs`** — metadata visibility (notes, handoff capsules)
- **`cli_dispatch_sync.rs`** — push/pull/sync progress and JSON output
- **`cli_workflows.rs`** — custom workflow install and runtime
- **`cli_state_hierarchy.rs`** — parent/child state cascading
- **`cli_skills.rs`** — managed `knots*` skill installation and doctor coverage
- **`repo_guardrails.rs`** — CLAUDE.md/AGENTS.md consistency, hook installation

## Running

```sh
cargo test --all-targets --all-features
make sanity  # fmt + lint + test + coverage
```

All tests use ephemeral temp directories. No external services required.
