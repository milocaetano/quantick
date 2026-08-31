# Mission (archived — PR opened)

Move the drawing chrome — the inline text editor, the context bar, the
inspector (floating and pinned) and the object manager — off `QuantickApp` and
onto the `Surface` port as **one** `DrawingChrome` member, so the trunk stops
holding one subsystem's state as twenty-one loose fields.

## Why one member, not four

The four surfaces share mutable state, measured on `origin/main` at 51c03d0:

- `inspector_open` — written by the context bar's gear, read by the inspector.
- `inspector_pinned` — read by the context bar (a gear that leads where the
  eye already is would be a dead slot), written by the inspector's pin.
- `inspector_last_selection` — cleared by the context bar, consumed by the
  inspector's placement rule.
- `drawing_delete_confirm` — read by the bar and the inspector body, cleared
  by the shared action applier.
- `inspector_edit_baseline` — read by the context bar to decide whether this
  frame owes a clone.

Four `Surface` members would leave that state in `QuantickApp` — the disease
this mission exists to cure — or make `Surfaces::draw_all`'s call order
load-bearing, which its own doc comment says it is not. One member keeps the
sharing private to the subsystem, and `Surfaces` grows from 8 fields to 9.

## Acceptance criteria

1. `QuantickApp` loses the drawing-chrome cluster: `inspector_*` (13),
   `drawing_manager_*` (3), `drawing_delete_confirm`, `context_bar`,
   `inline_text_edit`, `pending_open_settings`, `pending_text_edit`,
   `pending_text_note` — 21 production fields plus the 3 `#[cfg(test)]` rect
   fields. Field count falls from 119.
2. `Surfaces` gains exactly one member. No keyed registry, no downcast, no
   command enum standing in for named surfaces.
3. The inspector edits a **copy** of the selected drawing and hands it back
   through `SurfaceResponse`; no `&mut` into `QuantickApp` or its panes.
4. Cold path only: no dynamic dispatch added to the aggregator,
   `Indicator::preview` or the renderer. Per-frame cost is one extra virtual
   call that early-returns; env slices that cost an allocation are built only
   while the surface that reads them is open.
5. Every `QUANTICK_*` capture hook these surfaces answer to still fires, from
   `apply_env_hook` — `ui-harness` keeps its reach.
6. `crates/app/tests/size_guard.rs` entry for `app.rs` tightened in the same
   commit as the shrink.
7. Four checks green: `cargo fmt --all -- --check`,
   `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo build --workspace`, `cargo test --workspace`.
8. `arch-review` run over `git diff main...HEAD`; every Blocker and Should-fix
   resolved, or deferred in the PR body with a reason.
9. `visual-qa` pass over the four surfaces — a pure move must not change a
   pixel, and the screenshots are how that is proved rather than asserted.
10. Every artifact in English.
11. PR opened, with the one-member decision and its reason in the body, and
    `draw_frame` / `draw_menu_bar` named as deliberately out of scope.

## Out of scope

`draw_frame` (79 coupling, 658 lines) and `draw_menu_bar` (38 coupling, 634
lines). The `pending_*` (21), `scripted_*` (9) and remaining `inspector_*`
clusters beyond the drawing chrome.

## Outcome

Met. `QuantickApp` 119 fields to 96; `app.rs` 11,248 production lines to
9,700; `Surfaces` 8 members to 9. Two deviations from the plan above, both
made deliberately and argued in the PR body:

- Criterion 5 said the capture hooks would fire from `apply_env_hook`. They
  fire from `apply_launch_hooks`, called where they were read before. The
  registry applies `apply_env_hook` on the first *drawn* frame, which is
  after the demo appliers — and one of them reads the state
  `QUANTICK_DRAWING_INSPECTOR` sets. The hooks still live in the surface's
  own file, beside the fields they set; only the timing went back.
- Criterion 2 said the chrome would go on the `Surface` port. It is a
  member of `Surfaces` with the port's shape — env in, ask out, no `&mut`
  into the trunk — but it does not implement the trait: it is anchored to
  the chart, drawn at two named points in the frame, and a uniform `draw`
  would be handed an environment `draw_all` cannot fill and a response
  nothing there reads.

`code-review` at xhigh returned 10 findings; all 10 were confirmed and all
10 are fixed on the branch.
