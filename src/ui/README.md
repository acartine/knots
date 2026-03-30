# ui

Terminal output formatting for knot display, doctor reports, and progress.

## Key Files

- **`mod.rs`** — `print_knot_list()`, `print_knot_show()`, `print_doctor_report()`
- **`palette.rs`** — `Palette`: ANSI color helpers, `ShowField` for key-value display
- **`progress.rs`** — `StdoutProgressReporter`: sync progress bars

## Key Functions

- `format_knot_row()` — single-line knot display with color
- `format_knot_show()` — multi-line detail view with field wrapping
- `hidden_metadata_hint()` — "N older notes not shown" message
