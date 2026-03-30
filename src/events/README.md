# events

Event file I/O. Writes JSON event files to `.knots/events/` and `.knots/index/`.

## Key Files

- **`mod.rs`** — `EventWriter`, `write_event()`, `write_index_event()`
- **`error.rs`** — `EventWriteError` for I/O and serialization failures

## Event Layout

- Full events: `.knots/events/YYYY/MM/DD/<uuid>-<type>.json`
- Index events: `.knots/index/YYYY/MM/DD/<uuid>-idx.knot_head.json`

Index events are lightweight summaries enabling fast sync without full event transfer.
