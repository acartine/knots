# write_dispatch

Maps CLI write commands to operations and executes them through the write queue.

## Key Files

- **`operation_map.rs`** — `operation_from_command()`: CLI args to `QueuedOperation`
- **`execute/mod.rs`** — `execute_operation()`: dispatches operations to App methods
- **`execute/execute_write_ops.rs`** — individual write operation handlers
- **`helpers.rs`** — shared formatting and output helpers

## Flow

```
CLI args -> operation_from_command() -> write_queue -> execute_operation() -> App methods
```

All writes serialize through the FIFO queue to prevent concurrent modification.
