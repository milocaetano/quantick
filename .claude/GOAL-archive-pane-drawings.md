# GOAL — drawings in indicator sub-panes

Let the trader draw in indicator sub-panes (CVD, RSI-like rows) the same way
they draw on the price plot, with band-correct anchoring and no loss of the
pane's existing pan/zoom/collapse gestures.

Branch `feat/pane-drawings`, worktree `../quantick-worktrees/feat-pane-drawings`.

## Design authority

The UX spec was decided by the trader-UX specialist (session 2026-08-07), not
by the user. Its rulings are binding for this goal; deviations are recorded
here with a reason.

Key rulings:

- A **band** is a region of a chart pane owning a value axis: the price band
  plus one per expanded indicator pane. `Drawing` gains `band: DrawingBand`
  (`Price` default, `Indicator(PaneKey)`, `AllBands`).
- `PaneKey { kind, ordinal }`, never `SlotId` — remove/re-add must re-adopt.
- One carve per frame yields `(rect, PriceScale)` per band; placement,
  hit-test, drag and paint all consume that same scale, built from
  `view.scale.resolve(view.last_auto)` (the resolved range, not the auto one).
- Every registered tool works in every band. Refusal is band *state*
  (collapsed, warming up, gutter, live lane), announced by cursor before the
  press.
- Time-only tools (`vertical_line`, `date_range`) belong to no band: one
  object, painted as clipped segments through every band.
- Value-bearing drawings never cross bands, and hit-testing never crosses
  bands.
- `AllCharts` shares a band drawing only to the same `PaneKey` on the other
  chart pane of the tab; it never crosses into another value space.
- Drawings are parked (not deleted) when their indicator goes away; input
  changes leave them alone with no amber.
- Prerequisite: an armed drawing tool consumes the **primary button only** —
  pan, wheel zoom, disclosure and divider keep working (audit S2 / blocker
  B-1).

## Acceptance criteria

1. Band model: `DrawingBand` + `PaneKey` on `Drawing`, default `Price`;
   existing objects, tests and screenshots unchanged.
2. One band carve per frame shared by placement, hit-test, drag and paint;
   a test proves curve and drawing move together under manual pane zoom.
3. Every tool available in every band; refusal by band state with the
   `NotAllowed` cursor and the collapsed-strip hover string.
4. Time-only tools become one `AllBands` object painted through every band —
   one store item, one manager row, one delete.
5. Armed tool consumes primary clicks only; disclosure, divider, pan and
   gutter zoom still win their pixels (test on registration order).
6. Visual language: drawable-band accent hairline, per-band clipping,
   off-band caret, band name in the inspector title, manager grouped by band.
7. Park-on-removal and re-adopt by `PaneKey`; sub-pane ruler readout drops
   `pts` and `%`; magnet uses the band's own plotted values plus zero.

## Injected gates

- Four checks green after rebasing on latest `main`.
- Performance impact declared per touched path (per-frame work: band carve,
  paint, hit-test) — classified in the plan, evidence in the PR body.
- Hot path touched ⇒ `APP_HEALTH_SUMMARY` fps/frame_avg under a dense tape vs
  a `main` control run, numbers in the PR body.
- User-visible ⇒ `ui-harness` hook (`QUANTICK_DRAWING_DEMO`) added in the same
  change; `visual-qa` over the 15-state matrix; `trader-ux-review` with no
  unresolved Blocker.
- Adds a capability ⇒ `new-extension`: port named, registration-only edits,
  defaults preserve today's behaviour, blast radius in the PR body.
- `arch-review` over `git diff main...HEAD`, every Blocker/Should-fix resolved
  or deferred in the PR body.
- PR opened. Merging is never part of the goal.

## Out of scope (name in the PR body)

Persistence across restart; surviving a bar-spec/feed/symbol switch
(`clear_overlay` still wipes both bands); overlay indicators as a distinct
band; moving a drawing between bands; alerts on a band level; auto-adding a
missing indicator for an `AllCharts` share; cross-band snapping.
