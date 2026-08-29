# Mission — axis pointer compass

**Objective:** Give the plain pointer a compass — an opt-in, per-axis mouse
tracker that marks the pointer's price on the price axis and the hovered bar's
time on the time axis, toggled from each axis's own right-click menu — and tag
the price axis with the level of every horizontal-line drawing.

Why it matters: the bars are atemporal (tick / volume / dollar), so a candle
carries no time a trader can read off the grid. Pointing at a candle has to be
enough to know *when* it happened and *at what price*, without arming the
crosshair tool.

## Scope

1. **Pointer axis tracker.** While the pointer is over the chart, a small tick
   plus a compact tag on the price axis reads the price under the pointer, and
   the same on the time axis reads the hovered bar's time. No lines across the
   canvas — that is the crosshair tool's job, and this is deliberately quieter.
   The marks vanish the moment the pointer leaves the chart.
2. **Two independent switches**, one per axis, offered in that axis's own
   right-click menu ("Track pointer price" / "Track pointer time"), persisted
   like every other layer, shipped **on** in `config/chart-layers.toml`.
3. **Horizontal-line price tags.** A horizontal line and a horizontal ray mark
   their own level on the price axis, in the drawing's colour, the way
   ProfitChart does. They ride the `Drawings` layer: hide the drawings and the
   tags go with them.
4. **Data honesty.** The time tag names a bar's time only where a bar is under
   the pointer. Over the projection margin past the live edge, or over the live
   lane, there is no bar and therefore no time — the tag is not drawn rather
   than extrapolated.

## Acceptance criteria

### Mission-specific

- [ ] A1 — With the tracker on, moving the pointer over the candles paints a
      price tag on the price axis at the pointer's height and a time tag on the
      time strip at the hovered bar's x, both matching what the axes' own
      labels would read at that pixel. Proven by a unit test over the same
      scale/viewport the frame paints from, not by eye.
- [ ] A2 — Pointer off the chart → neither tag is painted. Pointer over the
      projection margin (no bar) → price tag yes, time tag no.
- [ ] A3 — Each axis's right-click menu carries its own toggle, and flipping it
      changes only that axis's tag. Test drives the menu, not the field.
- [ ] A4 — The two switches persist across a restart through the existing
      chart-layer store, and a fresh install (shipped `chart-layers.toml`)
      opens with both on.
- [ ] A5 — A horizontal line and a horizontal ray each paint a price tag on the
      axis at their own level, in their own colour; moving the drawing moves
      the tag; hiding the `Drawings` layer hides both.
- [ ] A6 — The axis-tag port has a fake second implementation under test, so a
      future tool docks by declaring a level rather than by editing the axis.
- [ ] A7 — The crosshair tool's existing tags and the new tracker never paint
      two price tags over each other.

### Standard gates

- [ ] G1 — English throughout (CLAUDE.md rule; `language_guard` + arch-review
      dimension 8).
- [ ] G2 — `cargo fmt --all -- --check`, `cargo clippy --workspace
      --all-targets -- -D warnings`, `cargo build --workspace`, `cargo test
      --workspace` all green on a branch rebased on latest `main`.
- [ ] G3 — Performance impact declared: every touched path classified by rate.
      This is **per-frame** work, so it needs numbers, not a belief.
- [ ] G4 — Hot-path evidence: `APP_HEALTH_SUMMARY` fps / frame_avg under a
      dense tape vs. a `main` control run, measured before the PR, numbers in
      its body.
- [ ] G5 — `ui-harness`: every new surface reachable from a fresh launch with
      no clicks — a pointer-position hook, `QUANTICK_CONTEXT_MENU=time` for the
      time axis's new menu, and the two switches through
      `QUANTICK_CHART_LAYERS`.
- [ ] G6 — `visual-qa` pass: every surface PASS or the defect explicitly
      accepted.
- [ ] G7 — `trader-ux-review` with no unresolved Blocker.
- [ ] G8 — `new-extension`: the axis-mark port named, tool edits
      registration-only, blast radius (added vs. edited files) in the PR body.
- [ ] G9 — Second operator: the two switches and the pointer readout are
      readable and settable by name through the control plane, not only by
      mouse.
- [ ] G10 — `arch-review` run over `git diff main...HEAD`, every Blocker and
      Should-fix resolved or deferred in the PR body.
- [ ] G11 — PR opened with green CI and the evidence in its body. Merging is
      not part of this mission.
