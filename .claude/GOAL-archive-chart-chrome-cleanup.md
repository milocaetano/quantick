# GOAL (archived) — chart chrome cleanup: nothing overlaps, everything can be switched off

**Branch**: `feat/chart-chrome-cleanup` · **PR**: #143 (open, CI green)
**Closed**: 08/08/2026

## Objective

Clean up the chart canvas chrome so nothing overlaps anything else, every
element drawn on the canvas can be switched off from the right-click menu, the
liquidity-map toggle reads as a book, and the live lane and live strip stop
depending on the aggression-bubbles switch.

## Verdict per criterion

- **A1 — right-click switches for the key and the badge — PASS.**
  `ChartLayer::FlowLegend` / `ChartLayer::BookStatus`, hints included,
  persisted through `chart-layers.toml`. Evidence: the layer-menu tests that
  iterate `ChartLayer::ALL`; `the_legend_draws_on_its_own_pass_and_the_trader_
  can_silence_it`.
- **A2 — lane and strip survive the bubbles being hidden — PASS.** The
  projection stopped applying the display switches; demand is stated by the
  pane. Evidence: `hiding_the_bubble_layer_keeps_the_clusters_in_the_frame`,
  `the_live_strip_alone_keeps_the_aggression_pipeline_running`,
  `the_strip_and_the_lane_marks_each_keep_the_projection_alive`.
- **A3 — the key survives it too — PASS.** Moved out of `draw_aggressions`
  into `draw_legend`; same test as A1.
- **A4 — book glyph and one visual language — PASS.** `icons::ROWS` for the
  depth map (toolbar and dock share it), a sideways histogram for the strip.
  Evidence: `the_layer_toggles_speak_one_visual_language`, asserting the
  glyphs the buttons actually use.
- **A5 — no canvas chrome overlaps — PASS with one limit.** The corner is a
  measured stack (header + HUD + one row per chip) with a single owner for the
  HUD offset, and the key stands down past half the canvas rather than
  climbing back. Evidence: `the_legend_starts_below_the_corner_it_was_told_
  about`, `the_predicted_stack_height_covers_what_the_legend_actually_draws`.
  The limit: proven by test and by reading, not by screenshot — see G4.
- **G1 — four checks green — PASS.** fmt, clippy `-D warnings`, build, and
  `cargo test --workspace` (exit 0, 47 suites) at HEAD, on top of current
  `origin/main`. CI green on the PR twice.
- **G2 — performance declared and measured — PASS.** Per-rate table in the PR
  body; three alternated runs per binary. `frame_cpu_ms` 2.41 (main) vs 2.40
  (branch); `heatmap_projection_ms` 0.67 vs 0.46. fps is not comparable on an
  idle desktop and is stated as such.
- **G3 — harness hooks — PASS.** Both new layers reachable through
  `QUANTICK_CHART_LAYERS`; the skill's hook table now documents the file's
  shape and the ids.
- **G4 — visual-qa — NOT RUN.** The capture desktop was idle (~58 min), so the
  window does not present and every capture returns a single colour
  (`fps=19 / frame_avg=52 ms`, distinct sampled colours = 1). Known
  environment state, not a render regression: the app ran correctly (book
  `live`, 1049 bid / 947 ask levels, clusters projecting). The matrix is
  prepared and stated in the PR body as unmet.
- **G5 — trader-ux-review — PASS.** No Blockers; all three Should-fix fixed
  (the split/focus overlap, the badge silencing a dead book, lane marks
  reading as on while nothing drew), two of five Consider fixed, the rest
  deferred in the PR body.
- **G6 — arch-review — PASS.** Its Blocker (encoding damage) was already
  repaired before the PR opened, and its suggested grep guard is now in the
  tree. Six Should-fix and two Consider fixed in `253a07d`; the rest deferred
  with reasons in the PR body.
- **G7 — PR open with the evidence — PASS.** #143, CI green.

## What this goal cost that was not planned

A `Set-Content` pass over three source files rewrote them in cp1252 with a BOM
and turned every em dash into mojibake — 48 of the damaged lines were UI
strings the trader reads. Found while reviewing my own diff stat, repaired
byte-for-byte in `964db21`, and now guarded by
`crates/app/tests/source_encoding_guard.rs`, because no other gate in the repo
can see it.
