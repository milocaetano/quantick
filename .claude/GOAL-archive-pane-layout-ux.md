# Goal

Make the indicator pane band below the chart hold up at any window size:
nothing rendered below the height at which it can be read, manual control over
the split, and a time axis that measures its own labels instead of counting
them.

Branch `feat/pane-layout-ux`, worktree `../quantick-worktrees/feat-pane-layout-ux`,
stacked on `feat/pane-live-lane` (PR #130) — both change `plot_split`,
`split_panes` and `draw_pane`, so cutting from `main` would conflict in exactly
those functions.

## The defects this fixes

- **D1 — the time axis counts labels instead of measuring them.**
  `pane.rs:draw_time_strip` uses `step = (visible / 6).max(1)`: always ~6
  labels whatever the width. An `HH:MM:SS` in monospace 10px is ~50 px, and the
  history strip is narrower than the chart because the live lane takes its
  share — so on a smaller window the labels collide. The price axis already
  solved this with `AXIS_LABEL_MIN_GAP_PX`; two axes, two rules.
- **D2 — pane height is a bare fraction with no floor.**
  `PANE_HEIGHT_FRAC = 0.20` of the body *per pane*, up to `MAX_PANES = 3`, so
  three panes take 60% of the chart at every size and each one still has to
  hold a rule, a title, a headline value, gridlines and a curve.
- **D3 — panes are the only band that cannot be resized.** The canvas split has
  a draggable divider and so does the live lane. Same product, three answers.

## Scope (user's call, taken 2026-08-06)

1. **Automatic: floor + collapse.** A pane is never drawn below a readable
   floor. When they do not all fit, the extras collapse to a labelled strip
   (name + live value) that expands on click.
2. **Manual: draggable dividers** between the candles and the pane stack and
   between panes, overriding the automatic layout, with the floor still
   enforced and double-click returning to automatic.
3. **Time axis stays in the footer**, with labels spaced by measured pixels and
   a format that degrades `HH:MM:SS` → `HH:MM` → `MM` as the strip narrows.

Out of scope: moving the time axis under the candles (considered and rejected —
the pane stack shares that x axis); persisting pane heights to disk across
restarts (the indicator state file stores slots, not geometry; that belongs
with the `ui-state.toml` goal).

## Acceptance criteria

### Specific to this goal

- [ ] No pane is ever painted below the readable floor, at any window size the
      app allows (min 900x560) and at any pane count up to `MAX_PANES`.
- [ ] A pane that does not fit collapses to a strip that still reports its name
      and its live value — collapsing hides the curve, never the number.
- [ ] Expanding a collapsed pane is one click, and the affordance says so
      without a tooltip.
- [ ] The divider between the candles and the pane stack, and between panes,
      drags; the floor holds during the drag; double click returns that
      divider to automatic.
- [ ] Time labels never overlap: spacing is decided by measured pixel width
      with a minimum gap, and the format degrades before the labels do.
- [ ] A chart with no pane indicators lays out exactly as it does today.

### Standard gates (any code change)

- [ ] Four checks green after rebasing on the parent branch: `cargo fmt --all --
      --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo build --workspace`, `cargo test --workspace`.
- [ ] **Performance impact declared** per touched path: layout split and label
      measurement are **per frame** and must stay O(panes) and O(labels); the
      divider drag is **per frame**, O(1); no per-trade or per-depth path is
      touched. Text measurement per frame is the one real cost — it is cached
      or bounded, and the choice is stated.
- [ ] `arch-review` over the diff, every Blocker/Should-fix resolved or
      deferred in the PR body.
- [ ] PR opened. Merging is not part of this goal.

### Hot path (per-frame layout and paint)

- [ ] `APP_HEALTH_SUMMARY` `frame_cpu_ms` against a control build of the parent
      branch, load-normalised (frame cost tracks `heatmap_cells`), numbers in
      the PR body.

### User-visible

- [ ] **New `ui-harness` hook: `QUANTICK_WINDOW_SIZE=WxH`.** Without it no
      agent can reproduce the reported defect at all — the window size is the
      trigger. Registered in the skill's table in the same change.
- [ ] A hook reaches the collapsed-pane state directly, not only by shrinking
      the window.
- [ ] `visual-qa` over the state matrix (1/2/3 panes x large/small window x
      collapsed/expanded x dragged/automatic). **See the known gap below.**
- [ ] `trader-ux-review` with no unresolved Blocker, and the design pass it
      produces is what gets built — the user asked for a professional UX
      opinion to lead this, not to rubber-stamp it.

## Known gap, stated up front

`visual-qa` by screenshot does not work in this environment: the app's chart
area captures white for agent-launched builds, and a `main` control build does
the same, so it is the environment rather than a regression. For a goal whose
whole point is that something looks bad, that matters more than usual. The
mitigation is to ask the user for a before/after screenshot at a small window
size, and to say plainly in the PR that nobody has looked at it if they do not
arrive.

## Status

Delivered as PR #132 (stacked on #130).

Met: nothing painted below the readable floor at any window the app allows;
panes that do not fit collapse to a strip that still reports name and live
value; expanding is one click with a visible affordance; dividers drag with the
floor holding and double click returns to automatic; time labels are spaced by
measured pixels and the format degrades before the labels do; a chart with no
panes lays out as before. Four checks green. `QUANTICK_WINDOW_SIZE` added and
registered. arch-review found a per-frame allocation, fixed rather than
deferred. **Visual QA ran** — the environment problem was an idle desktop, not
the machine — and caught a tofu chevron no test would have.

Partly met: the performance comparison is **inconclusive** at n=4 per arm
(+0.29 ms, 0.79 sd). Reported as inconclusive rather than rounded to flat.

Seen and left alone (pre-existing, out of scope): at 900x560 the floating
legends overlap the candles and the status bar's segments collide.
