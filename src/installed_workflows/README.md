# installed_workflows

Workflow and profile loading, parsing, and validation.

## Key Files

- **`mod.rs`** — public API and re-exports
- **`loader.rs`** — `load_profiles()`: discover and load workflow bundles
- **`operations.rs`** — `install_workflow()`, `uninstall_workflow()`
- **`bundle_toml.rs`** / **`bundle_json.rs`** — TOML and JSON bundle parsing
- **`profile_toml.rs`** / **`profile_json.rs`** — profile definition parsing
- **`builtin.rs`** — built-in workflow bundle loading and prompt rendering

## Key Types

- `ProfileDefinition` — states, transitions, owners, gates for a workflow variant
- `WorkflowTransition` — allowed state transitions
