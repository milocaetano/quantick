# AGENTS.md — quantick for AI agents

Real-time alternative-bar charts (tick / volume / dollar / imbalance) for order
flow trading, in Rust. [`CLAUDE.md`](CLAUDE.md) owns the working rules and the
verification loop; [`docs/README.md`](docs/README.md) indexes the docs,
[`docs/agentic-development.md`](docs/agentic-development.md) the reasons behind
the rules. This file is the crate map.

**Driving the running app instead of changing the code?** Quantick ships its
own MCP server: build `quantick-mcp`, run `quantick-mcp setup --client claude`
(or `codex`), enable **Tools → Local agent access**, then call
`quantick_describe` first; the rest is discoverable from its answer.
Tools, profiles and authority: [`crates/mcp/README.md`](crates/mcp/README.md).
Capability IDs, versions and permissions: the generated
[capability inventory](docs/control-plane/capability-inventory.md) and
[capability catalog](schemas/control/observer-capability-catalog-v1.json).

## The map

A Cargo workspace under `crates/`. *Depends on* is one-way, enforced by
`crates/guards/src/graph.rs`; never add a reverse edge. `app`, `backtest`,
`mcp` and `guards` are leaves; `guards` has no edges either way.

| Crate | Depends on | What it owns |
| --- | --- | --- |
| `app` | every crate but the other leaves and `trading` | The desktop chart (egui). |
| `backtest` | strategy, pine, indicators, replay, sim, paper (dev), engine | Recorded sessions in, performance out, over the chart's engine and indicator path. |
| `mcp` | control-local, control | The MCP adapter; stdout carries MCP frames only. |
| `guards` | — | The size, context, cycle and UI-free ratchets, the English and encoding scans. |
| `pine` | indicators | "Quantick Pine", a Pine v5 subset; zero external dependencies. |
| `strategy` | sim, engine | Armed price regions, projected brackets, the armed-instance state machine, `SignalAlarm`. |
| `control-local` | control | Instance-descriptor directory and blocking loopback client; one ownership check serves both. |
| `control-host` | control | Projection registry, admission, idempotency store, event journal. Told the time. |
| `indicators` | engine | The `Indicator` trait (commit/preview with rollback), incremental `ta.*` kernels, draw objects. |
| `replay` | engine | Recorded sessions: CSV format, folder scan, playback clock. Told the time. Test support is the documented `replay::test_support` module, not a feature. |
| `paper` | sim, engine, civil | One paper account — orders, risk sizing, journal, report — three drivers. |
| `sim` | trading, engine | `TradingVenue` implementation; fills only on what the tape proves, never on quotes. |
| `trading` | engine | Venue-neutral order vocabulary and the `TradingVenue` port. |
| `feed` | feed-*, replay, orderbook, engine | The `FeedEvent`/`FeedCommand` port and its adapters, feed config, history reach, session exporter. Owns runtimes, threads and the clock. |
| `feed-binance`, `feed-hyperliquid`, `feed-mt5` | engine, orderbook | Venue sources; produce trades. |
| `orderflow` | engine, orderbook | Liquidity history, grouping, timeline, settled/live heatmap projections. Told the time. |
| `engine` | — | Trades in, bars out. Deterministic, no clock. |
| `orderbook` | — | L2 book core: validated snapshots, absolute level updates, update-id continuity. |
| `control` | — | Control-plane contracts, and the `fake` host/client ports, published on purpose. |
| `civil` | — | Civil dates and the display offset the journal, report and axis share. |
