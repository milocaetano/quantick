# Goal

Make the live lane an **independent pane**: a band pinned to the right edge of
the chart that always shows the most recent trades, with its own zoom over
market time and a draggable divider, whatever the candles behind it are doing.

The lane stops riding the viewport. Panning, zooming or any other chart
movement no longer moves it, shrinks it or empties it — "eu estaria sempre
vendo os trades mais recentes, não importa se eu arrastar o gráfico ou fizer
qualquer movimento". Because the tape can no longer be lost, the "back to
live" badge is removed.

## Acceptance criteria

1. **Pinned.** The lane is a fixed band at the chart's right edge. Panning the
   candles left/right or zooming the candle width never changes the lane's
   x range, its span of market time, or what is drawn in it.
   *Proof: a layout test mapping the same lane position under two different
   pan/zoom states to the same screen x.*
2. **Its own zoom.** Dragging left/right (or scrolling) on the bottom time
   strip **under the lane** changes only the lane's time window; the same
   gesture left of the divider changes only the candle spacing.
   *Proof: `live_lane.zoom` scales the window, unit-tested; the two strip
   segments are separate interactions.*
3. **Zooming out aggregates.** A smaller lane zoom shows more market time and
   merges more prints into each bubble — the lane's clustering window scales
   with its span, so a cluster keeps a constant width on screen.
   *Proof: unit test on `LiveLaneStyle::effective_cluster_ms`.*
4. **Resizable divider.** Hovering the divider shows the horizontal-resize
   cursor; dragging it sets `live_lane.width_share`, clamped between 5 % and
   50 % of the chart — the lane can never take more than half.
   *Proof: `MAX_LIVE_LANE_SHARE == 0.5` plus a test on the drag conversion.*
5. **No "back to live" badge.** The symbol is gone from the codebase; the
   status bar still says how to get back to live.
6. **Persisted and additive.** The lane zoom is a preset field that round-trips
   through `bubbles.toml` and a slider in the panel; its default reproduces
   today's window exactly.
7. **Green loop.** `cargo fmt --all -- --check`, `cargo clippy --workspace
   --all-targets -- -D warnings`, `cargo build --workspace`, `cargo test
   --workspace`, plus `arch-review` over `git diff main...HEAD` with no
   unresolved Blocker or Should-fix.
