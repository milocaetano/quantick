# quantick

**Real-time alternative bar charts for order flow trading.**
Tick, volume, dollar and imbalance bars from live market data — a Rust desktop app with a bot-ready engine.

> ⚠️ Early development. The design is being worked out in the open; nothing here is usable yet.

## Why

Time-based candles distort order flow. A 1-minute bar at the session open and one at lunch contain wildly different amounts of trading, yet look the same on your chart. Sampling by *activity* instead of time — every N trades, every N contracts, every $N of notional, or every time flow imbalance spikes — produces bars with far better statistical properties, and shows the market the way flow traders actually read it.

Almost no charting platform offers these bar types, and no open-source tool builds them from a live feed.

## What quantick will be

- **Bar engine (Rust)** — trades in → tick / volume / dollar / imbalance bars out, with per-bar delta and CVD built in
- **Live feed** — Binance first (public data, works out of the box); MetaTrader 5 planned
- **Desktop app** — native chart (egui/wgpu) showing bars form in real time
- **Bot-ready** — the same engine that draws your chart feeds your strategy: the bars your bot trades are byte-identical to the bars you backtested

## Design principles

1. **One engine, three consumers.** Chart, backtest and bot consume the same aggregator. Live/backtest parity by construction, not by discipline.
2. **Deterministic.** Same trades in, same bars out. Always.
3. **Data honesty.** Inferred or incomplete data is labeled, never silently patched.

## Roadmap

- [ ] Core bar engine (tick / volume / dollar bars)
- [ ] Binance aggTrades feed
- [ ] Desktop chart
- [ ] Imbalance bars
- [ ] CVD & delta visuals
- [ ] Python bindings
- [ ] MetaTrader 5 feed

## License

MIT
