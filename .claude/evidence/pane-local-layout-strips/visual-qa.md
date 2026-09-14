# Visual QA: pane-local layout strips

Verdict: **PASS**. No unaccepted visual defect remains in the changed surfaces.

## Scope

The pass covered the layout footer in a default split, distinct assignments,
live dense market data, a narrow split at the twelve-layout limit, the
pane-owned rename editor, and the shared delete confirmation. The chart body,
pane divider, status bar, time header, tool rail, price axes, live strip, and
feed badges were treated as affected neighbours because the footer changes
their available rectangle.

## State matrix

| Surface / state | Verdict | Evidence |
| --- | --- | --- |
| Normal 1400x900 split over live BTC data | PASS | `wide-distinct-layouts.png`: both panes have separate footers; the left pane marks `Levels` and the right marks `Layout 1`; divider, axes, status bar and charts do not overlap. Health held 59-60 fps with about 1.6 ms frame CPU and zero worker backlog. |
| Feature active with distinct assignments | PASS | `wide-distinct-layouts.png` and `every_visible_pane_keeps_its_own_layout_strip_when_focus_moves`: the complete shared catalogue is repeated while each pane keeps its own active marker. |
| Narrow 700x700 split, twelve shared layouts | PASS | `narrow-max-layouts.png`: each footer clips at its own pane boundary, remains horizontally scrollable, and automatically reveals its own active layout (`Scalping` left, `Session` right). The disabled `+` retains its existing explanatory hover text. Health held 59-60 fps with about 1.2 ms frame CPU and zero worker backlog. |
| Rename editor over live data | PASS | `rename-origin-pane.png`: the editor appears only in the right pane footer that owns the transient edit; the left footer remains readable and stable. It does not cover price, tape, or the forming bar. |
| Delete confirmation over live data | PASS | `delete-confirmation.png`: the shared destructive action retains its explicit confirmation, layout name, drawing consequence, and Delete/Cancel pair. The live price and right-edge tape remain visible. |
| Empty or unavailable market data | PASS (not data-dependent) | Footer composition reads the shared `LayoutBook` and pane assignment only. `the_layout_strip_costs_each_pane_its_own_footer` exercises empty geometry; no feed capability gates or placeholder claims changed. |
| Motion sanity | PASS by structured and interaction evidence | Live sessions reported healthy frames and changing trade/book counters without layout movement. `every_visible_pane_keeps_its_own_layout_strip_when_focus_moves` proves focus changes preserve both footer rectangles and active assignments. A quiet 1.3-second live sample produced identical pixels, so it was not used as proof of tape motion. |

## Structured evidence

`control-health.jsonl` records a hook-driven control-plane capture from the
same changed application surface. Evidence
`VJgmOFi-xaM50T743vrGJw` has digest
`sha256:db5c2cc43a4c394f7dbe81d49352918be473c11b4c3f2d432a4dd4813959e5a7`,
three chunks, fourteen captured scopes, and a screenshot. The following
health summary stabilized at 59 fps, 16.67 ms wall frame time, 1.62 ms CPU,
and zero backlog, parked, deferred, coalesced, or blocked worker work.

## Defect checklist

- Integrity: PASS; every footer remains within its pane and all neighbouring
  splitters and controls remain intact.
- Readability: PASS; tab labels use the existing status-bar type scale and the
  active fill/rule remain distinct on the chrome background.
- Occlusion: PASS; footers reserve space instead of overlaying market data.
- State honesty: PASS; active assignments remain per-pane, while rename and
  delete visibly retain shared-catalog semantics.
- Consistency: PASS; the existing tab, confirmation, and hover language is
  reused rather than introducing a new control dialect.
- Performance: PASS; visual sessions stayed at 59-60 fps, and the paired dense
  frame fixture was flat-to-better versus `main` (see `validation.md`).
