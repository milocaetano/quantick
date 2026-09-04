# quantick-orderflow

The order-flow engine behind the chart's heatmap, bubbles and live lane, with
nothing of the chart in it:

- `history` — the authoritative `LiquidityHistory`: honest coverage of the L2
  book over time, resting-liquidity runs, aggressions, tape age.
- `grouping` — price grouping of displayed liquidity into visual runs.
- `timeline` — bars and the live edge laid out as a `BarTimeline`.
- `projection` — the settled half of the chart projected once and cached, and
  the live half rebuilt on every print (`project_settled`, `project_live`).
- `interaction` — liquidity events and aggression clusters correlated with the
  book.
- `scale` — the session price scale.
- `config` — `HeatmapConfig` and the bubble, lane and theme knobs the chart
  persists.
- `engine` — `BookEngine`: the thread-free state machine that owns the history,
  the synchronization status, the diagnostic counters and the projection cache,
  driven synchronously by whoever holds it.

It depends on `quantick-engine` (bars, trades) and `quantick-orderbook` (depth
events) and on nothing above them. No egui, no channels, no network, no wall
clock: the projection cache is *told* the time by its caller through
`BookEngine::project_at`, so a test drives it with any instant it likes. The
two `Instant::now()` reads that remain are stopwatches measuring how long the
settled and live halves took; their numbers reach the diagnostic counters and
never an output.

The desktop chart is the first consumer: its `orderflow_worker` owns one
`BookEngine` on a dedicated thread, and `orderflow_view` and `orderflow_render`
draw the published projections. `backtest` may consume it next — that is why
it is a crate rather than a module — and nothing here would change for it to.
