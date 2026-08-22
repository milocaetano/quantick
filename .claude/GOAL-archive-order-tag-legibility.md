# Mission — the order labels stop fighting the price

**Objective**: the cmd-trading aim label rides the pointer instead of parking
at the chart's right edge, and a resting order's in-plot tag rests as a
compact pill that only opens to its full text under the pointer — so a trader
clicks without travelling and reads the candles without a banner over them.

Branch: `feat/order-tag-legibility`
Worktree: `../quantick-worktrees/feat-order-tag-legibility`

## Design decisions (confirmed with the user)

1. **Aim label**: follows the pointer's x, anchored just *left* of the cursor
   with a fixed gap — never under it. Clicking still needs a deliberate micro
   move, so a held modifier plus a stray click cannot fire an order.
2. **Resting order tag**: compact pill at rest (`STP 1`, no ✕); expands to
   `#6 BUY STP 1 @ 180615 ✕` while hovered. The price already lives in the
   gutter chip, so the resting form never repeats it.

## Acceptance criteria

### Mission-specific

- [ ] **A1** — `cmd_preview_layout` takes the pointer's x: moving the pointer
      left moves the label left. Unit test asserts the label's centre tracks
      the pointer across several x positions.
- [ ] **A2** — the label never sits under the cursor (a fixed gap separates
      them) and never leaves the interactive band: clamped at the band's left
      and right edges, with the dashed line still reaching the right edge.
      Unit test covers both extremes.
- [ ] **A3** — paint and press share one geometry (the overlay-controls
      rule): the same `cmd_preview_layout` call feeds `draw_cmd_preview` and
      the hit-test, proven by a test that presses the painted rect.
- [ ] **A4** — a resting order's tag paints the compact form (kind + qty, no
      ✕) while nothing hovers it, and the full form (`#id SIDE KIND qty @
      price ✕`) while the line, the pill or the dock row is hovered. Shape-dump
      test asserts both.
- [ ] **A5** — the ✕ is pressable *only* while the full form is painted: the
      hit-test shares the paint's hover predicate, so no invisible ✕ is
      clickable. Test presses the ✕ rect in the resting state and asserts
      nothing is cancelled.
- [ ] **A6** — a dragged order keeps its full form (mid-drag readout is the
      one time the trader needs every field), matching today's behaviour.

### Standard gates (any code change)

- [ ] **G1** — four checks green after rebasing on latest `main`:
      `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --
      -D warnings`, `cargo build --workspace`, `cargo test --workspace`.
- [ ] **G2** — performance impact declared. Touched paths classified by rate:
      `cmd_preview_layout`, `draw_cmd_preview`, `chip_tag` and the order loop
      in `draw_layer` are **per-frame**; nothing per-trade or per-depth is
      touched. Budget: no new per-frame allocation, no galley laid out that
      was not laid out before.
- [ ] **G3** — hot path evidence, not belief: `APP_HEALTH_SUMMARY`
      fps/frame_avg under a dense tape vs. a `main` control run, numbers in
      the PR body.
- [ ] **G4** — `arch-review` run over `git diff main...HEAD`, every Blocker
      and Should-fix resolved or deferred in the PR body.
- [ ] **G5** — PR opened. Merging is not part of the mission.

### Standard gates (user-visible change)

- [ ] **U1** — `ui-harness`: every new/changed surface reachable by env hook,
      added in this same change. Needed: the aim label at a chosen pointer x,
      and a resting order's tag in both the compact and the hovered form.
      `QUANTICK_CMD_PREVIEW` and `QUANTICK_PAPER_DEMO` exist; the pointer x
      and the forced order hover do not.
- [ ] **U2** — `visual-qa` pass with every surface PASS or defects explicitly
      accepted. State matrix: aim label at left / centre / right of the plot,
      near the top and bottom edges; resting tag compact vs. hovered; with and
      without an open position under it.
- [ ] **U3** — `trader-ux-review` with no unresolved Blocker.

## Out of scope

Horizontal parking of the tag, a density setting in the Trading tab, and any
change to the order-entry form. The user chose hover-expand over both; adding
them would be scope creep.
