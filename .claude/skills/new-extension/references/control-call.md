# Control call — one declaration

Exemplar: `workspace_book_calls!` in `crates/control-handlers/src/wire/workspace_book.rs`.

A call is one entry in its family's exported table macro: `id` (const and
string), `handler`, `module`, `input`, `result`, `descriptor` (a builder plus
its prose), optional `readback` (a `crate::readback` constructor without the
id) and optional `ui` (title, reach, keys, `with` for calls the same door also
performs). Nothing else names it.

- The schema module expands the table with `crate::calls::declare_calls!`:
  ID consts, descriptor fns, `READBACKS`, `UI_ROWS`.
- The handler module expands it with a local consumer matching
  `id`, `handler`, then `$($declared:tt)*`, registering each descriptor.
- `UI_ROWS` joins the matrix in `crates/control-handlers/src/wire/ui_behaviours.rs`.

Adding a call: the entry plus its handler body; regenerate the docs and
`schemas/control`. An `Optional` readback also needs its payload row in the
retry proof its `proven_by` names. A new wire type adds its `documents()` row.
