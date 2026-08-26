# Mission

A fixed-range volume profile must not restyle the candles, the chart window
must resize freely down to nothing, and a time chart must open on one week of
venue candle history with an explicit way to load older.

## Context

Three defects the trader reported after PR #234 (`fix/frvp-never-freezes`):

1. Dropping a fixed-range volume profile (FRVP) on a freshly loaded chart
   changed the candles themselves — odd colour, different size, spacing that
   grew with zoom — and turning the footprint layer on "fixed" it. Root cause
   found: `ChartPane::draw_chart` computes `footprint_on` as the *ladder
   accumulation* switch (`footprint_visible || wants_range_profile()`), and
   then uses that same flag to decide the candle's sidebar lane and its
   fade/outline dressing (`crates/app/src/pane.rs`, `candle_lane` and
   `faded_candles`). The layer's own paint is gated on
   `layer_visible(ChartLayer::Footprint, ..)` instead. So an FRVP drawing —
   which only wants the ladders, never the layer — dresses every candle for a
   footprint that is never painted. Introduced by `68c3618 feat(app): fixed
   range volume profile drawing`.
2. The window refuses to be dragged below `MIN_WINDOW_PX` (900x560,
   `crates/app/src/main.rs`).
3. A time pane opens on ninety days of venue candles
   (`TIME_HISTORY_SPAN_MS`, `crates/app/src/feed/mod.rs`) with no way to reach
   further back afterwards — `Tab::request_ohlcv_history` refuses once
   `ohlcv_base` is set, and the toolbar's `+ older` pages *trades*, not
   candles.

4. The volume profile looked wrong on the time chart and right on the tick
   (flow) chart with L2 off. Investigated: the profile's own paint is
   *identical* on both panes — the silhouette-over-heatmap path needs a
   heatmap, and with L2 off neither pane has one, so both draw the same
   translucent histogram (0.55 alpha inside the value area, 0.30 outside).
   What differed was the candles under it, which is defect 1. Carried as a
   criterion so it is *verified* rather than assumed.
5. Reported, not yet reproduced: a gap where loaded history meets current
   data, seen on WINV26 via MetaTrader. A Binance time pane with real venue
   history shows a continuous seam (captured), so this is not the shared
   `trim_to_seam` path. Awaiting the trader's answer on which of three
   shapes it is: one giant bar at the seam, missing bars, or a price jump.

## Acceptance criteria

1. **Candles are untouched while the footprint layer is hidden.** The candle
   lane and the fade/outline dressing follow the same predicate the layer's
   paint does, not the accumulation switch. One owner for that predicate, used
   at both sites.
2. **Accumulation still follows the FRVP.** Placing or drafting a range
   profile keeps `set_footprint_enabled(true)` and the throttled ladder
   snapshot running, so the profile still folds.
3. **Both proved by test**: a pane holding an FRVP with the layer hidden
   reports "do not dress the candles" and "do accumulate"; with the layer shown
   it reports both true.
4. **The window resizes to nothing.** No `with_min_inner_size` floor, no clamp
   on the saved or env-requested size, and a degenerate window draws a frame
   without panicking (test over the layout entry point).
5. **A time pane opens on one week** of venue candle history, not ninety days.
6. **Older candle history is loadable on demand** and prepends in front of what
   is held, for every provider that serves candles; the action is drivable
   without a mouse (named action + env hook) and labelled honestly when a feed
   has nothing older to give.
7. **The profile reads the same on both panes** with L2 off — verified from
   a capture, not from reading the code.
8. **Standard gates**: `cargo fmt --all -- --check`, `cargo clippy --workspace
   --all-targets -- -D warnings`, `cargo build --workspace`, `cargo test
   --workspace` all green on top of latest `main`; performance impact declared
   (per-frame candle path, rare window/history paths); `arch-review` run with
   every Blocker/Should-fix resolved or deferred in the PR body; `visual-qa`
   pass over the affected surfaces; `trader-ux-review` with no unresolved
   Blocker; every artifact in English; PR opened.
