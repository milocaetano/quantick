# GOAL — chart chrome cleanup: nothing overlaps, everything can be switched off

**Branch**: `feat/chart-chrome-cleanup`
**Worktree**: `F:\src\quantick-worktrees\feat-chart-chrome-cleanup`
**Kind**: code change · user-visible · per-frame draw paths · no new capability

## Objective

Clean up the chart canvas chrome so nothing overlaps anything else, every
element drawn on the canvas can be switched off from the right-click menu, the
liquidity-map toggle reads as a book, and the live lane and live strip stop
depending on the aggression-bubbles switch.

## Source (user annotations on a live BTCUSDT · Binance screenshot, 07/08/2026)

1. The liquidity-map legend (`liquidity / buy aggression / sell aggression /
   aggression-aligned depletion / L2 reduction (unattributed) / L2 gap`) clutters
   the canvas and has no way out — it must be switchable from the right-click
   menu, like every other canvas layer.
2. The EMA indicator chip "rola sobre a posição" — top-left chrome overlaps
   (indicator chip vs. liquidity-map legend, which draw in the same corner).
3. "A parte da Live Strip tem que ser separada das bolhas de agressão. Se eu
   desativar as bolhas de agressão, quero continuar vendo essa parte."
   Confirmed scope: **both** the live lane inside the canvas and the live strip
   beside the price axis stay alive with `aggression bubbles` off.
4. The top-right toolbar icons are incoherent with each other; the liquidity-map
   toggle in particular must read as a **book / bid-ask ladder (lists)**.

Decided with the user when the goal was set:
- right-click gains switches for the **liquidity-map legend** and the **book
  status badge**; indicator chips stay as they are (their overlap is fixed as a
  layout defect, not by a new switch).

## Acceptance criteria

### Feature
- [ ] A1 — The right-click canvas menu has an entry for the liquidity-map legend
      and one for the book status badge, in the existing `chart layers` section,
      each with a hint saying what stops being drawn and what keeps running.
      Both persist through `chart-layers.toml` like the other layers.
- [ ] A2 — Turning `aggression bubbles` off leaves the **live lane** (its marks
      and its own live-edge content) and the **live strip** (histogram + bid/ask
      touch) drawing and computing. Proven by a test that paints with bubbles off
      and asserts both surfaces still emit, not only by a screenshot.
- [ ] A3 — The liquidity-map legend keeps working with the bubbles layer off too:
      it is chrome about the map, not about the bubbles (today it is painted
      inside `draw_aggressions`).
- [ ] A4 — The top-right toolbar toggle for the liquidity map uses a book/ladder
      glyph (stacked bid/ask rows), and the whole top-right icon set is brought to
      one visual language (same stroke weight, grid, metaphor family).
- [ ] A5 — No canvas chrome overlaps: indicator chips, liquidity-map legend,
      status badge, footprint legend and the last-price chip each own their space
      at the window sizes the harness can drive (including the small window).

### Gates (quantick standard)
- [ ] G1 — `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
      -- -D warnings`, `cargo build --workspace`, `cargo test --workspace` all
      green after rebasing on latest `main`.
- [ ] G2 — Performance impact declared per touched path by rate (per-frame draw
      code here); `APP_HEALTH_SUMMARY` fps/frame_avg under a dense tape compared
      against a `main` control run, numbers in the PR body.
- [ ] G3 — `ui-harness`: every new/changed surface reachable by env hook, hooks
      added in this same change.
- [ ] G4 — `visual-qa` pass over the state matrix (bubbles on/off × legend on/off
      × badge on/off × small/large window), every surface PASS or the defect
      explicitly accepted.
- [ ] G5 — `trader-ux-review` with no unresolved Blocker — this is the "UX
      designer" the user asked for, and it must specifically report on overlap.
- [ ] G6 — `arch-review` over `git diff main...HEAD`, every Blocker and
      Should-fix resolved or deferred in the PR body.
- [ ] G7 — PR opened with the evidence in its body. Merging is not part of the
      goal.

## Out of scope
- Redesigning the L2 side panel itself.
- Touching the footprint, drawings or paper-trading surfaces beyond fixing an
  overlap they cause.
- Any change to what is recorded or computed by the capture — display only.
