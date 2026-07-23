# quantick

**Real-time alternative bar charts for order flow trading — and one engine to take your research from chart to backtest to bot.**

> ⚠️ Early development. The design is being worked out in the open; nothing here is usable yet. Star/watch the repo if you want to follow along.

## Why this exists

Time-based candles distort order flow. A 1-minute bar at the session open and a 1-minute bar during lunch look identical on your chart, yet one may contain fifty times more trading than the other. Every indicator you compute on top inherits that distortion: a delta of +500 contracts means one thing in a quiet bar and something completely different in a busy one.

The fix has been known for decades: sample by **activity** instead of time. Close a bar every N trades (tick bars), every N contracts (volume bars), every $N of notional (dollar bars), or whenever buying/selling pressure gets unusually one-sided (imbalance bars). The research literature — from Ané & Geman (2000) to López de Prado's *Advances in Financial Machine Learning* — documents why this works: activity-sampled bars have far better statistical properties, and they make flow metrics comparable from bar to bar.

**Quant funds and professional desks use these bar types every day.** But the tooling has stayed locked up: it lives inside proprietary platforms (Bookmap, ATAS, Sierra Chart, NinjaTrader) that don't expose an engine you can program against, or inside private codebases that never see daylight. If you want to watch these bars form live on a chart — and then hand the *exact same bars* to a backtest or a trading bot — there is essentially no open-source option today.

quantick exists to change that: **a free, open, programmable implementation of the charts professionals actually use, built for the community.**

## What quantick is for

1. **Visualize.** A native desktop app that renders tick, volume, dollar and imbalance bars from a live market feed — with per-bar delta and CVD built in. See the market the way flow traders read it.
2. **Research.** Study setups directly on the charts: how does absorption look on volume bars? Where does CVD diverge? Chart-driven analysis is where strategy ideas are born.
3. **Build.** The same engine that draws your chart feeds your backtests and your bots. The bars your strategy trades live are byte-identical to the bars you researched and backtested — parity by construction, not by discipline.

## Candle appearance

Open **🎨 candle** from the chart toolbar to tune candle rendering without
changing bars, feeds or order-book capture. The default **Order flow** preset
uses a low-opacity body and a strong directional outline so liquidity and
aggression remain visible.

- Presets: **Order flow**, **Glass**, **Outline only** and **Classic**.
- Independent bull/bear colours for the body fill and outline.
- Body fill opacity, outline opacity/thickness, body width, corner radius and
  minimum doji height.
- Optional wicks with directional or custom colour, opacity and thickness.
- A forming-candle opacity control.
- **Outline only** removes the candle body fill entirely; this is the clearest
  mode for dense heatmaps.
- Canvas background and grid can be recoloured, faded or disabled independently.

The settings window includes a live preview. Candle paint is intentionally
layered after resting liquidity and before aggression bubbles:
`heatmap → candle → aggression`. Appearance changes only trigger a redraw and
never restart the market-data pipelines.

## Optional L2 heatmap

The chart can capture Binance Spot level-2 order-book depth and render a Bookmap-style liquidity heatmap. It is **disabled by default** and must be enabled from the chart controls.

The visualization follows a few data-honesty rules:

- History begins at the first successfully synchronized live snapshot/update sequence. Binance does not provide historical L2 backfill through this feed, so candles before that point are marked as unavailable instead of being reconstructed.
- Depth update IDs are checked continuously. A disconnect, sequence gap or resynchronization closes the current liquidity runs, marks the affected interval with subtle shading and dashed vertical boundaries, and starts again from a fresh snapshot. Stale book state is never stretched across a gap.
- Heatmap quantities are resting bid/ask amounts from the snapshot plus absolute depth updates, limited to the configured number of price levels on each side. Liquidity outside that coverage is unknown.
- Optional `aggTrade` bubbles show confirmed market aggression as a factual overlay. They never subtract quantity from the book: a depth reduction can be an execution, cancellation, replacement or a combination of those events.
- Captured history is bounded and kept in memory only. Restarting the application starts a new capture.

The chart exposes these settings:

| Setting | Default | Range / behavior |
| --- | ---: | --- |
| L2 heatmap | Off | Starts live capture when enabled |
| Retention | 30 minutes | 1–1,440 minutes |
| Price bucket | `0.01` | Any positive value; changing it resets heatmap history and restarts capture |
| Opacity | `0.72` | `0.0`–`1.0` |
| Gamma | `0.75` | `0.1`–`3.0` |
| Intensity scale | Visible P99 | Automatic visible-window P99 or a fixed full-intensity quantity |
| Aggression bubbles | On | Can be hidden independently of the heatmap |

The in-memory safety budgets are 500,000 liquidity runs (approximately 64 MiB), 100,000 aggression records, 50,000 projected visible cells and 2,000 projected bubbles. Old history is pruned and excess render primitives are dropped within those limits; the associated counters are emitted in diagnostic logs. The exact RLE history remains independent from rendering: visible projection is refreshed at the 100 ms depth cadence and its heat cells are submitted as one batched mesh.

### L2 and logging environment variables

| Variable | Default | Behavior |
| --- | --- | --- |
| `QUANTICK_BOOK_DEPTH` | `1000` | Binance snapshot depth per side. Numeric values are clamped to `1`–`5000`; a missing or invalid value uses the default. Higher values increase initial REST payload, synchronization work and memory use. |
| `QUANTICK_LOG_FORMAT` | `text` | Set to `json` for newline-delimited JSON diagnostic logs on stderr. |
| `RUST_LOG` | `quantick=info` | Standard tracing filter; for example, use `quantick=debug` for deeper diagnostics. |

JSON logs include stable fields such as `schema_version`, `event_code`, symbol, connection generation, update IDs, recovery action and health counters so synchronization and coverage gaps can be investigated without inferring state from prose.

## Who it's for

- **Flow traders** who want professional bar types without platform lock-in
- **Bot developers** who need deterministic, programmable bar construction for strategies driven by order flow
- **Quant researchers** who want reproducible activity-sampled bars for backtesting and ML feature engineering

## Planned architecture

- **Bar engine (Rust)** — raw trades in → alternative bars out; deterministic and headless, usable with no UI attached
- **Live feed** — Binance first (public data, works out of the box, no API key needed); MetaTrader 5 planned
- **Desktop app** — native chart (egui/wgpu) showing bars form in real time
- **Bindings** — Python bindings planned, so the engine plugs into existing backtest stacks

## Design principles

1. **One engine, three consumers.** Chart, backtest and bot consume the same aggregator. Live/backtest divergence is a bug class we design out, not test out.
2. **Deterministic.** Same trades in, same bars out. Always.
3. **Data honesty.** Inferred or incomplete data is labeled, never silently patched.
4. **Small and focused.** This is not a trading platform. It builds and shows bars, and exposes them to your code. That's the job.

## Roadmap

- [ ] Core bar engine (tick / volume / dollar bars)
- [ ] Binance aggTrades feed
- [ ] Desktop chart
- [ ] Imbalance bars (López de Prado information-driven sampling)
- [ ] CVD & delta visuals
- [ ] Python bindings
- [ ] MetaTrader 5 feed
- [ ] C API, so bots in C++ (or any language) can consume the engine
- [ ] GPU footprint / heatmap rendering (wgpu)

## Contributing

The whole point of this project is to open up tooling that has historically been private. Ideas, use cases and design discussion are welcome right now — [start a discussion](https://github.com/milocaetano/quantick/discussions), even before there's code to review. Ready to contribute code? See [CONTRIBUTING.md](CONTRIBUTING.md) for the workflow.

## License

MIT
